#[macro_use]
extern crate slog;
extern crate time;
extern crate portus;
extern crate rand;
extern crate rustfft;
extern crate num_complex;
use portus::{CongAlg, Config, Datapath, DatapathInfo, DatapathTrait, Report};
use portus::ipc::Ipc;
use portus::lang::Scope;
use rand::{thread_rng, ThreadRng, Rng};
use rustfft::FFT;
use num_complex::Complex;

pub struct Nimbus<T: Ipc> {
    control_channel: Datapath<T>,
    logger: Option<slog::Logger>,
    sc: Scope,
    sock_id: u32,
    mss: u32,

    rtt: time::Duration,
    ewmaRtt: f64,
    lastDrop: Vec<time::Timespec>,
    lastUpdate: time::Timespec,
    baseRTT: f64,
    waitTime: time::Duration,
    startTime: time::Timespec,

    uest: f64,
    alpha: f64,
    beta: f64,
    delayThreshold: f64,
    flowMode: String,
    delayMode: String,
    lossMode: String,
    rate: f64,
    ewmaRate: f64,
    cwnd: Vec<f64>,
    xtcpFlows: i32,
    ssthresh: Vec<f64>,
    cwndClamp: f64,

    initDelayThreshold: f64,
    frequency: f64,
    pulseSize: f64,
    zoutHistory: Vec<f64>,
    ztHistory: Vec<f64>,
    rttHistory: Vec<f64>,
    measurementInterval: time::Duration,
    lastHistUpdate: time::Timespec,
    lastSwitchTime: time::Timespec,
    useSwitching: bool,
    switchingThresh: f64,
    ewmaElasticity: f64,
    ewmaMaster: f64,
    ewmaSlave: f64,
    ewma_alpha: f64,

    lastBWEst: time::Timespec,
    indexBWEst:  Vec<i32>,
    rinHistory: Vec<f64>,
    routHistory: Vec<f64>,
    aggLastDrop: time::Timespec,
    BWEstMode: bool,
    maxRout: f64,
    ewma_rin: f64,
    ewma_rout: f64,
    useEWMA: bool,
    setWinCap: bool,

    masterMode: bool,
    switchingMaster: bool,
    r: ThreadRng,
    lastSlaveMode: time::Timespec,

    cubicInitCwnd: f64,
    cubicCwnd:  f64,
    cubicSsthresh: f64,
    cwnd_cnt: f64,
    tcp_friendliness: bool,
    cubic_beta: f64,
    fast_convergence: bool,
    C: f64,
    Wlast_max: f64,
    epoch_start: f64,
    origin_point: f64,
    dMin: f64,
    Wtcp: f64,
    K: f64,
    ack_cnt: f64,
    cnt: f64,

    pkts_in_last_rtt: f64,
    velocity: f64,
    cur_direction: f64,
    prev_direction: f64,
    prev_update_rtt: time::Timespec,
}

#[derive(Clone)]
pub struct NimbusConfig {
    pub useSwitchingArg: bool,
    pub bwEstModeArg: bool,
    pub delayThresholdArg: f64,
    pub xtcpFlowsArg: i32,
    pub initDelayThresholdArg: f64,
    pub frequencyArg: f64,
    pub pulseSizeArg: f64,
    pub switchingThreshArg: f64,
    pub flowModeArg: String,
    pub delayModeArg: String,
    pub lossModeArg: String,
    pub uestArg: f64,
    pub useEWMAArg: bool,
    pub setWinCapArg: bool,
    // TODO make more things configurable
}
impl Default for NimbusConfig {
    fn default() -> Self {
        NimbusConfig {
            useSwitchingArg: false,
            bwEstModeArg: true,
            delayThresholdArg: 1.25f64,
            xtcpFlowsArg: 2,
            initDelayThresholdArg: 1.25,
            frequencyArg: 5.0,
            pulseSizeArg: 0.25,
            switchingThreshArg: 0.4,
            flowModeArg: String::from("XTCP"),
            delayModeArg: String::from("Nimbus"),
            lossModeArg: String::from("MulTCP"),
            uestArg: 96.0 * 125000.0,
            useEWMAArg: false,
            setWinCapArg: false ,
        }
    }
}
impl<T: Ipc> Nimbus<T> {
    fn send_pattern(&self, mut rate: f64, waitTime: time::Duration) {
        if (time::get_time()-self.startTime).num_seconds() < 1 {
            rate = 2_000_000.0;
        }
        let mut win = rate;
        win = 15000f64.max(rate * 2.0 * (self.rtt.num_milliseconds() as f64) * 0.001);
        //match self.control_channel.send_pattern(
        //    self.sock_id,
        //    make_pattern!(
        //		pattern::Event::SetRateAbs(rate as u32) =>
        //		pattern::Event::SetCwndAbs(win as u32) =>
        //		pattern::Event::WaitNs(waitTime.num_nanoseconds().unwrap() as u32) =>
        //		pattern::Event::Report
        //    ),
        //) {
        //    Ok(_) => (),
        //    Err(e) => {
        //        self.logger.as_ref().map(|log| {
        //            warn!(log, "send_pattern"; "err" => ?e);
        //        });
        //    }
        //}
        self.control_channel.update_field(&self.sc, &[("Rate", rate as u32), ("Cwnd", win as u32)]);
    }

    fn install(&self, waitTime: time::Duration) -> Scope {
        self.control_channel.install(
            format!("
                (def (Report.acked 0) (Report.rtt 0) (Report.rin 0) (Report.rout 0) (Report.loss 0) (Report.timeout false))
                (when true
                    (bind Report.acked (+ Report.acked Ack.bytes_acked))
                    (bind Report.rtt Flow.rtt_sample_us)
                    (bind Report.rin Flow.rate_outgoing)
                    (bind Report.rout Flow.rate_incoming)
                    (bind Report.loss Ack.lost_pkts_sample)
                    (bind Report.timeout Flow.was_timeout)
                    (fallthrough)
                )
                (when (|| Report.timeout (> Report.loss 0))
                    (report) 
                    (reset)
                )
                (when (> Micros {})
                    (report)
                    (reset)
                )
            ", waitTime.num_microseconds().unwrap()).as_bytes()
        ).unwrap()
    }

    fn get_fields(&mut self, m: &Report) -> Option<(u32, u32, f64, f64, u32, bool)> {
        let sc = &self.sc;
        let acked = m.get_field(&String::from("Report.acked"), sc).map(|x| x as u32)?;
        let rtt = m.get_field(&String::from("Report.rtt"), sc).map(|x| x as u32)?;
        let rin = m.get_field(&String::from("Report.rin"), sc).map(|x| x as f64)?;
        let rout = m.get_field(&String::from("Report.rout"), sc).map(|x| x as f64)?;
        let loss = m.get_field(&String::from("Report.loss"), sc).map(|x| x as u32)?;
        let was_timeout = (m.get_field(&String::from("Report.timeout"), sc).map(|x| x as u32)?)==1;
        Some((acked, rtt, rin, rout, loss, was_timeout))
    }

}

impl<T: Ipc> CongAlg<T> for Nimbus<T> {
    type Config = NimbusConfig;

    fn name() -> String {
        String::from("nimbus")
    }

    fn create(control: Datapath<T>, cfg: Config<T, Nimbus<T>>, info: DatapathInfo) -> Self {
        let mut s = Self {
            sock_id: info.sock_id,
            control_channel: control,
            sc: Default::default(),
            logger: cfg.logger,
            mss: info.mss,

            useSwitching: cfg.config.useSwitchingArg,
            BWEstMode: cfg.config.bwEstModeArg,
            delayThreshold: cfg.config.delayThresholdArg,
            xtcpFlows: cfg.config.xtcpFlowsArg,
            initDelayThreshold: cfg.config.initDelayThresholdArg,
            frequency: cfg.config.frequencyArg,
            pulseSize: cfg.config.pulseSizeArg,
            switchingThresh: cfg.config.switchingThreshArg,
            flowMode: cfg.config.flowModeArg,
            delayMode: cfg.config.delayModeArg,
            lossMode: cfg.config.lossModeArg,
            uest: cfg.config.uestArg,
            useEWMA: cfg.config.useEWMAArg,
            setWinCap:  cfg.config.setWinCapArg,

            baseRTT: -0.001f64, //careful
            lastDrop: Vec::new(),
            lastUpdate: time::get_time(),
            rtt: time::Duration::milliseconds(300),
            ewmaRtt: 0.1f64,
            startTime: time::get_time(),
            ssthresh: Vec::new(),
            cwndClamp: 2e6 * 1448.0,

            alpha: 0.8f64,
            beta: 0.5f64,
            rate: 100000f64,
            ewmaRate: 10000f64,
            cwnd: Vec::new(),

            zoutHistory: Vec::new(),
            ztHistory: Vec::new(),
            rttHistory: Vec::new(),
            measurementInterval: time::Duration::milliseconds(10),
            lastHistUpdate: time::get_time(),
            lastSwitchTime: time::get_time(),
            ewmaElasticity: 1.0f64,
            ewmaSlave: 1.0f64,
            ewmaMaster: 1.0f64,
            ewma_alpha: 0.01f64,

            lastBWEst: time::get_time(),
            indexBWEst: Vec::new(),
            rinHistory: Vec::new(),
            routHistory: Vec::new(),
            aggLastDrop: time::get_time(),
            maxRout: 0.0f64,
            ewma_rin: 0.0f64,
            ewma_rout: 0.0f64,


            waitTime: time::Duration::milliseconds(5),

            masterMode: true,
            switchingMaster: false,
            r: thread_rng(),
            lastSlaveMode: time::get_time(),

            cubicInitCwnd: 10f64,
            cubicCwnd: 10f64,
            cubicSsthresh: ((0x7fffffff as f64)/ 1448.0),
            cwnd_cnt: 0f64,
            tcp_friendliness: true,
            cubic_beta: 0.3f64,
            fast_convergence: true,
            C: 0.4f64,

            pkts_in_last_rtt: 0f64,
            velocity: 1f64,
            cur_direction: 0f64,
            prev_direction: 0f64,
            prev_update_rtt: time::get_time(),

            Wlast_max: 0f64,
            epoch_start: -0.0001f64,
            origin_point: 0f64,
            dMin: -0.0001f64,
            Wtcp: 0f64,
            K: 0f64,
            ack_cnt: 0f64,
            cnt: 0f64,
        };

        for i in 0..s.xtcpFlows {
            s.cwnd.push(s.rate/(s.xtcpFlows as f64));
        }

        for i in 0..s.xtcpFlows {
            s.lastDrop.push(time::get_time());
        }

        for i in 0..s.xtcpFlows {
            s.ssthresh.push(s.cwndClamp);
        }

        //s.cubic_reset(); Careful
        s.sc = s.install(s.waitTime);
        s.send_pattern(s.rate, s.waitTime);

        s
    }


    fn on_report(&mut self, _sock_id: u32, m: Report) {
        let (acked, rtt, mut rin, mut rout, loss, was_timeout) =  self.get_fields(&m).unwrap();

        self.rtt = time::Duration::microseconds(rtt as i64);

        if loss>0 {
            self.handle_drop();
            return;
        }
        if was_timeout {
            self.handle_timeout();//Careful
            return;
        }

        let RTT = rtt as f64 * 0.000001;
        self.ewmaRtt = 0.95*self.ewmaRtt + 0.05*rtt as f64 * 0.000001;
        if self.baseRTT <= 0.0 || RTT < self.baseRTT {//careful
            self.baseRTT = RTT;
        }

        self.pkts_in_last_rtt = acked as f64 / 1448.0;
        let now = time::get_time();
        let elapsed = (now-self.startTime).num_milliseconds() as f64 * 0.001;
        //let mut  float_rin = rin as f64;
        //let mut float_rout = rout as f64; // careful

        self.ewma_rin = 0.2*rin + 0.8*self.ewma_rin;
        self.ewma_rout = 0.2*rout + 0.8*self.ewma_rout;

        if self.useEWMA {
            rin=self.ewma_rin;
            rout=self.ewma_rout;
        }

        if self.maxRout < self.ewma_rout {
            self.maxRout = self.ewma_rout;
            if self.BWEstMode == true {
                self.uest = self.maxRout;
            }
        }

        let mut zt = self.uest*(rin/rout) - rin;
        if zt.is_nan() {
            zt = 0.0;
        } 

        while now>self.lastHistUpdate {
            self.rinHistory.push(rin);
            self.routHistory.push(rout);
            self.zoutHistory.push(self.uest-rout);
            self.ztHistory.push(zt);
            self.rttHistory.push(self.rtt.num_milliseconds() as f64 * 0.001);
            self.lastHistUpdate = self.lastHistUpdate+self.measurementInterval;
        }

        if self.flowMode == "DELAY"{
            self.frequency = 6.0f64;
        self.updateRateDelay(rin, zt, acked as u64);
    } else if self.flowMode == "XTCP" {
        self.frequency = 5.0f64;
        self.updateRateLoss(acked as u64);
    }

    self.rate = self.rate.max(0.05*self.uest);

    if self.masterMode {
        self.rate =  self.elasticityEstPulse().max(0.05*self.uest);
    }
    self.send_pattern(self.rate, self.waitTime);
    self.shouldSwitchFlowMode();

    self.lastUpdate = time::get_time();

    self.logger.as_ref().map(|log| {
        debug!(log, "[nimbus] got ack"; 
               "ID" => self.sock_id,
               "baseRTT" => self.baseRTT,
               "currRate" => self.rate * 8.0,
               "currCwnd" => self.cwnd[0],
               "newlyAcked" => acked,
               "rin" => rin * 8.0,
               "rout" => rout * 8.0,
               "ewma_rin" => self.ewma_rin * 8.0,
               "ewma_rout" => self.ewma_rout * 8.0,
               "max_ewma_rout" => self.maxRout * 8.0,
               "zt" => zt * 8.0,
               "rtt" => RTT,
               "uest" => self.uest * 8.0,
               "elapsed" => elapsed,
               );
    });
    //n.lastAck = m.Ack Careful
    }
}

impl<T: Ipc> Nimbus<T> {
    fn handle_drop(&mut self) {
        if self.lossMode == "Cubic" {
            self.cubicDrop()
        } else {
            self.mulTcpDrop()
        }
        return;
    }

    fn cubicDrop(&mut self) {
        if (time::get_time() - self.lastDrop[0]) < self.rtt {
            return;
        }
        self.epoch_start = -0.0001f64; //careful
        if (self.cubicCwnd < self.Wlast_max) && self.fast_convergence {
            self.Wlast_max = self.cubicCwnd * ((2.0 - self.cubic_beta) / 2.0);
        } else {
            self.Wlast_max = self.cubicCwnd;
        }
        self.cubicCwnd = self.cubicCwnd * (1.0 - self.cubic_beta);
        self.cubicSsthresh = self.cubicCwnd;
        self.cwnd[0] = self.cubicCwnd * 1448.0;
        self.rate = self.cwnd[0]/(self.rtt.num_milliseconds() as f64 * 0.001);
        if self.flowMode == "XTCP" {
            self.send_pattern(self.rate, self.waitTime);
        }
        self.logger.as_ref().map(|log| {
            debug!(log, "[nimbus cubic] got drop"; 
                   "ID" => self.sock_id as u32,
                   "time since last drop" => (time::get_time()-self.lastDrop[0]).num_milliseconds() as f64 * 0.001,
                   "rtt" => self.rtt.num_milliseconds() as f64 * 0.001,
                   );
        });
        self.lastDrop[0] = time::get_time();
        self.aggLastDrop = time::get_time();
    }

    fn mulTcpDrop(&mut self) {
        let mut totalCwnd = 0.0;
        for i in  0..self.xtcpFlows{
            totalCwnd += self.cwnd[i as usize]
        }
        let mut j = rand::random::<f64>();
        let mut i = 0;
        for k in 0..self.xtcpFlows {
            j -= self.cwnd[k as usize] / totalCwnd;
            if j < 0.0 {
                i = k;
                break;
            }
        }


        if (time::get_time() - self.lastDrop[0]).num_milliseconds() as f64 * 0.001 < self.baseRTT {
            return;
        }

        self.cwnd[i as usize] /= 2.0;
        self.updateRatemulTcp(0);
        //not perfect
        self.ssthresh[i as usize] = self.cwnd[i as usize];
        self.rate = self.rate.max(0.05*self.uest);
        if self.flowMode == "XTCP" {
            self.send_pattern(self.rate, self.waitTime);
        }

        //if len(n.indexBWEst) > 1 && float64((len(n.rinHistory)-n.indexBWEst[len(n.indexBWEst)-1]))*n.measurementInterval.Seconds() < 2*n.rtt.Seconds() {
        //  n.indexBWEst = n.indexBWEst[:len(n.indexBWEst)-1]
        //} Careful

        self.logger.as_ref().map(|log| {
            debug!(log, "[nimbus XTCP] got drop"; 
                   "ID" => self.sock_id as u32,
                   "time since last drop" => (time::get_time()-self.lastDrop[0]).num_milliseconds() as f64 * 0.001,
                   "rtt" => self.rtt.num_milliseconds() as f64 * 0.001,
                   "xtcflows" => i,
                   );
        });

        self.lastDrop[i as usize] = time::get_time();
        self.aggLastDrop = time::get_time();

    }

    fn updateRateDelay(&mut self, rin: f64, zt: f64, newBytesAcked: u64) {
        let RTT = self.rtt.num_milliseconds() as f64 * 0.001;
        if self.delayMode == "Vegas" {
            let mut totalCwnd = 0.0;
            for i in  0..self.xtcpFlows{
                totalCwnd += self.cwnd[i as usize];
            }
            self.baseRTT=0.05;//careful
            let inQueue = totalCwnd*((RTT-0.05)/RTT); 
            if self.ewmaRtt<1.05*self.baseRTT && RTT<1.05*self.baseRTT{
                self.cwnd[0] += 0.1 * newBytesAcked as f64;
            } else if inQueue< 30.0 * 1448.0 {
                self.cwnd[0] += 1448.0 * (newBytesAcked as f64 / totalCwnd);
            } else if inQueue > 40.0 * 1448.0 {
                self.cwnd[0] -= 1448.0 * (newBytesAcked as f64 / totalCwnd);
            }
            totalCwnd = 0.0;
            for i in 0..self.xtcpFlows {
                totalCwnd += self.cwnd[i as usize];
            }
            if self.masterMode {
                self.rate = totalCwnd / RTT;
            } else {
                //n.lastSlaveMode = time.Now()
                //n.rate = totalCwnd / n.rtt.Seconds()
                self.rate = totalCwnd / self.ewmaRtt;
            }
        } else if self.delayMode == "COPA" {
            let mut increase = false;
            if (RTT * 1448.0) > ((RTT - 1.2 * self.baseRTT) * (1.9/2.0) * self.cwnd[0]) {
                increase = true;
                self.cur_direction += 1.0;
            } else {
                self.cur_direction -= 1.0;
            }
            if (time::get_time()-self.prev_update_rtt)>self.rtt {
                if (self.prev_direction > 0.0 && self.cur_direction > 0.0) || (self.prev_direction < 0.0 && self.cur_direction < 0.0) {
                    self.velocity *= 2.0;
                } else {
                    self.velocity = 1.0;
                }
                if self.velocity > 100000.0 {
                    self.velocity = 100000.0;
                }
                self.prev_direction = self.cur_direction;
                self.cur_direction = 0.0;
                self.prev_update_rtt = time::get_time();
            }
            let change = (self.velocity * 1448.0 * (newBytesAcked as f64)) / (self.cwnd[0] * (1.0/2.0));
            if increase {
                self.cwnd[0] += change;
            } else {
                if change + 15000.0 > self.cwnd[0] {
                    self.cwnd[0]=15000.0;
                } else {
                    self.cwnd[0] -= change;
                }
            }
            self.rate = self.cwnd[0]/RTT;
        } else {
            let delta = RTT;
            self.rate = rin + self.alpha * (self.uest - zt - rin) - ((self.uest * self.beta) / delta) * (RTT - (self.delayThreshold * self.baseRTT));
            if self.delayThreshold > self.initDelayThreshold {
                self.delayThreshold -= ((self.measurementInterval.num_milliseconds() as f64 * 0.001) / 0.1) * 0.05;
            }
        }
    }

    fn cubic_reset(&mut self) {
        self.Wlast_max = 0.0;
        self.epoch_start = -0.0001;
        self.origin_point = 0.0;
        self.dMin = -0.0001; //careful
        self.Wtcp = 0.0;
        self.K = 0.0;
        self.ack_cnt = 0.0;
    }

    fn updateRateLoss(&mut self, newBytesAcked: u64) {
        if self.lossMode == "Cubic" {
            self.updateRateCubic(newBytesAcked);
        } else {
            self.updateRatemulTcp(newBytesAcked);
        }
    }

    fn updateRateCubic(&mut self, newBytesAcked: u64) {
        let mut no_of_acks = (newBytesAcked as f64) / 1448.0;
        if self.cubicCwnd < self.cubicSsthresh {
            if (self.cubicCwnd + no_of_acks) < self.cubicSsthresh {
                self.cubicCwnd += no_of_acks;
                no_of_acks = 0.0;
            } else {
                no_of_acks -= (self.cubicSsthresh - self.cubicCwnd);
                self.cubicCwnd = self.cubicSsthresh;
            }
        }
        let RTT = self.rtt.num_milliseconds() as f64 * 0.001;
        for i in 0..no_of_acks as usize {
            if self.dMin <= 0.0 || RTT < self.dMin {
                self.dMin = RTT;
            }
            self.cubic_update();
            if self.cwnd_cnt > self.cnt {
                self.cubicCwnd = self.cubicCwnd + 1.0;
                self.cwnd_cnt = 0.0;
            } else {
                self.cwnd_cnt = self.cwnd_cnt + 1.0;
            }
        }
        self.cwnd[0] = self.cubicCwnd * 1448.0;
        let totalCwnd = self.cwnd[0];
        if self.masterMode {
            self.rate = totalCwnd / RTT;
        } else {
            self.rate = totalCwnd / self.ewmaRtt;
        }
        self.ewmaRate = self.rate;
    }

    fn cubic_update(&mut self) {
        self.ack_cnt = self.ack_cnt + 1.0;
        if self.epoch_start <= 0.0 {
            self.epoch_start = (time::get_time().sec as f64) + f64::from(time::get_time().nsec)/1_000_000_000.0;
            if self.cubicCwnd < self.Wlast_max {
                self.K = (0.0f64.max((self.Wlast_max-self.cubicCwnd)/self.C)).powf(1.0/3.0);
                self.origin_point = self.Wlast_max;
            } else {
                self.K = 0.0;
                self.origin_point = self.cubicCwnd;
            }
            self.ack_cnt = 1.0;
            self.Wtcp = self.cubicCwnd;
        }
        let t = (time::get_time().sec as f64) + f64::from(time::get_time().nsec)/1_000_000_000.0 + self.dMin - self.epoch_start;
        let target = self.origin_point + self.C*((t-self.K)*(t-self.K)*(t-self.K));
        if target > self.cubicCwnd {
            self.cnt = self.cubicCwnd / (target - self.cubicCwnd);
        } else {
            self.cnt = 100.0 * self.cubicCwnd;
        }
        if self.tcp_friendliness {
            self.cubic_tcp_friendliness();
        }
    }

    fn cubic_tcp_friendliness(&mut self) {
        self.Wtcp = self.Wtcp + (((3.0 * self.cubic_beta) / (2.0 - self.cubic_beta)) * (self.ack_cnt / self.cubicCwnd));
        self.ack_cnt = 0.0;
        if self.Wtcp > self.cubicCwnd {
            let max_cnt = self.cubicCwnd / (self.Wtcp - self.cubicCwnd);
            if self.cnt > max_cnt {
                self.cnt = max_cnt;
            }
        }
    }

    fn updateRatemulTcp(&mut self, newBytesAcked: u64) {
        let mut totalCwnd = 0.0;
        for i in 0..self.xtcpFlows {
            totalCwnd += self.cwnd[i as usize];
        }
        for i in 0..self.xtcpFlows {
            let mut xtcpNewByteAcked = (newBytesAcked as f64) * (self.cwnd[i as usize] / totalCwnd);
            if self.cwnd[i as usize] < self.ssthresh[i as usize] {
                if self.cwnd[i as usize] + xtcpNewByteAcked > self.ssthresh[i as usize] {
                    xtcpNewByteAcked -= self.ssthresh[i as usize] - self.cwnd[i as usize];
                    self.cwnd[i as usize] = self.ssthresh[i as usize];
                } else {
                    self.cwnd[i as usize] += xtcpNewByteAcked;
                    xtcpNewByteAcked = 0.0;
                }
            }
            self.cwnd[i as usize] += 1448.0 * (xtcpNewByteAcked / self.cwnd[i as usize]);
            if self.cwnd[i as usize] > self.cwndClamp {
                self.cwnd[i as usize] = self.cwndClamp;
            }
        }
        if self.masterMode {
            self.rate = totalCwnd / (self.rtt.num_milliseconds() as f64 * 0.001);
        } else {
            self.rate = totalCwnd / self.ewmaRtt;        
        }
        self.ewmaRate = self.rate
    }

    fn elasticityEstPulse(&mut self) -> f64 {
        let elapsed = (time::get_time()-self.startTime).num_milliseconds() as f64 * 0.001;
        let fr_modified = self.uest;
        let mut phase = elapsed * self.frequency;
        phase -= phase.floor();
        let upRatio = 0.25;
        if phase < upRatio {
            return self.rate + self.pulseSize * fr_modified * (2.0 * std::f64::consts::PI * phase * (0.5 / upRatio)).sin();
        } else {
            return self.rate + (upRatio/(1.0-upRatio)) * self.pulseSize * fr_modified * (2.0 * std::f64::consts::PI * (0.5 + (phase - upRatio) * (0.5 / (1.0 - upRatio)))).sin();
        }
    }

    fn switchToDelay(&mut self, rtt: time::Duration) {
        if !self.useSwitching {
            return;
        }
        if self.flowMode == "DELAY" || ((time::get_time() - self.lastSwitchTime).num_milliseconds() as f64 * 0.001) < 5.0 {
            return;
        }
        self.delayThreshold = self.initDelayThreshold.max((rtt.num_milliseconds() as f64 * 0.001 )/ self.baseRTT);

        self.logger.as_ref().map(|log| {
            debug!(log, "switched mode"; 
                   "ID" => self.sock_id,
                   "elapsed" => (time::get_time() - self.startTime).num_milliseconds() as f64 * 0.001,
                   "from" =>  self.flowMode.clone(),
                   "to" => "DELAY",
                   "DelayTheshold" => self.delayThreshold,
                   );
        });
        self.flowMode = String::from("DELAY");
        self.lastSwitchTime = time::get_time();
        self.velocity = 1.0;
        self.cur_direction = 0.0;
        self.prev_direction = 0.0;
        self.updateRateLoss(0);
    }

    fn switchToXtcp(&mut self, rtt: time::Duration) {
        if !self.useSwitching {
            return;
        }
        if self.flowMode == "XTCP" {
            return;
        }
        self.logger.as_ref().map(|log| {
            debug!(log, "switched mode"; 
                   "ID" => self.sock_id,
                   "elapsed" => (time::get_time() - self.startTime).num_milliseconds() as f64 * 0.001,
                   "from" =>  self.flowMode.clone(),
                   "to" => "XTCP",
                   );
        });
        self.flowMode = String::from("XTCP");
        self.rate = self.routHistory[self.routHistory.len() - ((5.0 / (self.measurementInterval.num_milliseconds() as f64 * 0.001)) as usize)];
        if self.lossMode == "Cubic" {
            self.epoch_start = -0.0001;//casreful
            self.cwnd[0] = self.rate * self.rtt.num_milliseconds() as f64 * 0.001;
            self.cubicCwnd = self.cwnd[0] / 1448.0;
            self.cubicSsthresh = self.cubicCwnd;
            self.K = 0.0;
            self.origin_point = self.cubicCwnd;
        } else {
            for i in 0..self.xtcpFlows {
                self.cwnd[i as usize] = self.rate * (self.rtt.num_milliseconds() as f64 * 0.001) / (self.xtcpFlows as f64);
                self.ssthresh[i as usize] = self.cwnd[i as usize];
            }
        }
        self.lastSwitchTime = time::get_time();
    }


    fn shouldSwitchFlowMode(&mut self) {
        let mut duration_of_fft = 5.0;
        if !self.masterMode {
            duration_of_fft = 2.5;
        }
        let T = self.measurementInterval.num_milliseconds() as f64 * 0.001;
        let mut N = (duration_of_fft / T) as i32;
        let mut i = 1;
        while true {
            if i >= N {
                N = i;
                break
            }
            i *= 2;
        };
        duration_of_fft = (N as f64) * T;
        if (time::get_time()-self.startTime).num_seconds() < 10 {
            return;
        }
        let end_index = self.ztHistory.len() - 1;
        let start_index = self.ztHistory.len() - ((duration_of_fft+1.0)/T) as usize;

        let raw_zt = &self.ztHistory.clone()[start_index..end_index];//careful: complexity
        let raw_rtt = &self.rttHistory.clone()[start_index..end_index];
        let raw_zout = &self.zoutHistory.clone()[start_index..end_index];

        let mut clean_zt: Vec<Complex<f64>> = Vec::new();//careful: complexity
        let mut clean_zout: Vec<Complex<f64>> = Vec::new();
        let mut clean_rtt: Vec<Complex<f64>> = Vec::new();

        for i in 0..N {
            if i as usize >= raw_rtt.len() {
                return;
            }
            let j = i as usize + 2 * ((raw_rtt[i as usize] / T) as usize);
            if j >= raw_zt.len() {
                return;
            }
            clean_zt.push(Complex::new(raw_zt[j], 0.0));
            clean_zout.push(Complex::new(raw_zout[i as usize], 0.0));
            clean_rtt.push(Complex::new(raw_rtt[i as usize], 0.0));
        }

        let avg_rtt = time::Duration::milliseconds((1000.0*self.mean_complex(&clean_rtt[(0.75*(clean_rtt.len() as f32)) as usize..])) as i64);
        let avg_zt = self.mean_complex(&clean_zt[(0.75 * (clean_zt.len() as f32)) as usize..]);

        clean_zt = self.detrend(clean_zt);
        clean_zout = self.detrend(clean_zout);

        let mut fft_zt_temp = FFT::new(clean_zt.len(), false);
        let mut fft_zt = clean_zt.clone();
        fft_zt_temp.process(&clean_zt[..], &mut fft_zt[..]);


        let mut fft_zout_temp = FFT::new(clean_zout.len(), false);
        let mut fft_zout = clean_zout.clone();
        fft_zout_temp.process(&clean_zout[..], &mut fft_zout[..]);

        let mut freq : Vec<f64> = Vec::new();
        for i in 0..((N/2) as usize) {
            freq.push(i as f64 * (1.0 / (N as f64 * T)));
        }

        let expected_peak = self.frequency;
        let mut expected_peak2 = self.frequency;
        if self.flowMode == "DELAY" {
            expected_peak2 = 5.0;
        } else {
            expected_peak2 = 6.0;
        }
        if self.masterMode {
            if avg_zt < 0.1 * self.uest {
                self.ewmaElasticity = 0.0;
            } else if avg_zt > 0.9*self.uest {
                self.ewmaElasticity = (1.0-self.ewma_alpha)*self.ewmaElasticity + self.ewma_alpha*6.0;
            }

            let (_, mean_zt) = self.findPeak(2.2*expected_peak, 3.8*expected_peak, &freq[..], &fft_zt[..]);
            let (exp_peak_zt, _) = self.findPeak(expected_peak-0.5, expected_peak+0.5, &freq[..], &fft_zt[..]);
            let (exp_peak_zout, _) = self.findPeak(expected_peak-0.5, expected_peak+0.5, &freq[..], &fft_zout[..]);
            let (other_peak_zt, _) = self.findPeak(expected_peak+1.5, 2.0*expected_peak-0.5, &freq[..], &fft_zt[..]);
            let (other_peak_zout, _) = self.findPeak(expected_peak+1.5, 2.0*expected_peak-0.5, &freq[..], &fft_zout[..]);
            let mut elasticity2 = fft_zt[exp_peak_zt].norm() / fft_zt[other_peak_zt].norm();
            let mut elasticity = (fft_zt[exp_peak_zt].norm() - mean_zt) / fft_zout[exp_peak_zout].norm();
            if fft_zt[exp_peak_zt].norm() < 0.25 * fft_zout[exp_peak_zout].norm() {
                elasticity2 = elasticity2.min(3.0);
                elasticity2 *= ((fft_zt[exp_peak_zt].norm()/fft_zout[exp_peak_zout].norm()) / 0.25).min(1.0);
            }
            self.ewmaElasticity = (1.0 - self.ewma_alpha) * self.ewmaElasticity + self.ewma_alpha * elasticity2;

            if (fft_zout[exp_peak_zout].norm() / fft_zout[other_peak_zout].norm()) < 2.0 {
                self.ewmaElasticity = (1.0 - self.ewma_alpha) * self.ewmaElasticity + self.ewma_alpha*3.0;
            }

            let (exp_peak_ztMaster, _) = self.findPeak(4.5, 6.5, &freq[..], &fft_zt[..]);
            let (exp_peak_zoutMaster, _) = self.findPeak(4.5, 6.5, &freq[..], &fft_zout[..]);
            self.ewmaMaster = (1.0 - 2.0 * self.ewma_alpha) * self.ewmaMaster + 2.0 * self.ewma_alpha * (fft_zt[exp_peak_ztMaster].norm() / fft_zout[exp_peak_zoutMaster].norm());

            if (time::get_time()-self.startTime).num_seconds() < 15 {
                return;
            }

            if self.ewmaElasticity > 2.25 {
                self.switchToXtcp(avg_rtt);
            } else if self.ewmaElasticity < 2.0 {
                self.switchToDelay(avg_rtt);
            }
            if self.ewmaMaster > 2.0 {
                self.switchToSlave();
            }

            self.logger.as_ref().map(|log| {
                debug!(log, "elasticityInf"; 
                       "ID" => self.sock_id,
                       "ZoutPeakVal" => fft_zout[exp_peak_zout].norm(),
                       "ZtPeakVal" => fft_zt[exp_peak_zt].norm(),
                       "elapsed" => (time::get_time() - self.startTime).num_seconds(),
                       "Elasticity" => elasticity,
                       "Elasticity2" => elasticity2,
                       "EWMAElasticity" => self.ewmaElasticity,
                       "EWMAMaster" => self.ewmaMaster,
                       "Expected Peak" => expected_peak,
                       );
            });
        } else {
            let (exp_peak_zout, _) = self.findPeak(expected_peak-0.5, expected_peak+0.5, &freq[..], &fft_zout[..]);
            let (exp_peak_zout2, _) = self.findPeak(expected_peak2-0.5, expected_peak2+0.5, &freq[..], &fft_zout[..]);
            self.ewmaSlave = (1.0 - 2.0 * self.ewma_alpha) * self.ewmaSlave + 2.0 * self.ewma_alpha * (fft_zout[exp_peak_zout2].norm() / fft_zout[exp_peak_zout].norm());

            let (exp_peak_zoutSlave, _) = self.findPeak(4.5, 6.5, &freq[..], &fft_zout[..]);
            let (other_peak_zoutSlave, _) = self.findPeak(7.0, 15.0, &freq[..], &fft_zout[..]);
            self.ewmaElasticity = (1.0 - self.ewma_alpha) * self.ewmaElasticity + self.ewma_alpha * (fft_zout[exp_peak_zoutSlave].norm() / fft_zout[other_peak_zoutSlave].norm());

            if (time::get_time() - self.startTime).num_seconds() < 15 {
                return;
            }
            if self.ewmaSlave  > 1.25 {
                self.ewmaSlave = 0.0;
                if self.flowMode == "DELAY" && avg_zt > 0.1 * self.uest{
                    self.switchToXtcp(avg_rtt);
                } else {
                    self.switchToDelay(avg_rtt)
                }
            }

            if self.ewmaElasticity < 1.5 {
                self.switchToMaster()
            }

            self.logger.as_ref().map(|log| {
                debug!(log, "elasticityInf"; 
                       "ID" => self.sock_id,
                       "ZoutPeakVal" => fft_zout[exp_peak_zout].norm(),
                       "Zout2PeakVal" => fft_zout[exp_peak_zout2].norm(),
                       "elapsed" => (time::get_time() - self.startTime).num_seconds(),
                       "EWMAElasticity" => self.ewmaElasticity,
                       "EWMASlave" => self.ewmaSlave,
                       "Expected Peak" => expected_peak,
                       "Expected Peak2" => expected_peak2,
                       );
            });
        }
    }

    fn switchToMaster(&mut self) {
        if !self.switchingMaster {
            return;
        }
        if self.r.gen::<f64>() < (0.005 * (self.ewma_rin / self.uest)) {
            self.logger.as_ref().map(|log| {
                debug!(log, "Switch To Master"; 
                       "ID" => self.sock_id,
                       "EWMAElasticity" => self.ewmaElasticity,
                       "elapsed" => (time::get_time() - self.startTime).num_seconds(),
                       "EWMASlave" => self.ewmaSlave,
                       );
            });
            self.masterMode = true;
            //n.ewmaElasticity = 3.0
            self.ewmaMaster = 1.0;
        }
    }

    fn switchToSlave(&mut self) {
        if !self.switchingMaster {
            return;
        }
        if self.r.gen::<f64>() < 0.005 {
            self.logger.as_ref().map(|log| {
                debug!(log, "Switch To Slave"; 
                       "ID" => self.sock_id,
                       "EWMAElasticity" => self.ewmaElasticity,
                       "EWMAMaster" => self.ewmaMaster,
                       "elapsed" => (time::get_time() - self.startTime).num_seconds(),
                       );
            });
            self.ewmaSlave = 0.0;
            self.masterMode = false;
            //n.ewmaElasticity = 3.0
        }
    }

    fn findPeak(& self, start_freq: f64, end_freq: f64, xf: &[f64], fft: &[Complex<f64>]) -> (usize, f64) {
        let mut max_ind = 0 as usize;
        let mut mean = 0.0;
        let mut count = 0.0f64;
        for j in 0..xf.len() {
            if xf[j] <= start_freq {
                max_ind = j;
                continue;
            }
            if xf[j] > end_freq {
                break;
            }
            mean += fft[j].norm();
            count += 1.0;
            if fft[j].norm() > fft[max_ind].norm() {
                max_ind = j;
            }
        }
        return (max_ind, mean /count.max(1.0));
    }

    fn mean_complex(& self, a: &[Complex<f64>]) -> f64 {
        let mut mean_val = 0.0;
        for i in 0..a.len() {
            mean_val += a[i].re;
        }
        return mean_val / (a.len() as f64);
    }

    fn detrend(& self, a: Vec<Complex<f64>>) -> Vec<Complex<f64>> {
        let mean_val = self.mean_complex(&a[..]);
        let mut b : Vec<Complex<f64>> = Vec::new();
        for i in 0..a.len() {
            b.push(Complex::new(a[i].re - mean_val, 0.0));
        }
        return b;
    }

    fn handle_timeout(&mut self) {
        self.handle_drop();
    }
}


