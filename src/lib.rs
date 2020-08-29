#[macro_use]
extern crate slog;
extern crate failure;
extern crate num_complex;
extern crate portus;
extern crate rand;
extern crate rustfft;
extern crate structopt;
extern crate time;

use failure::bail;
use num_complex::Complex;
use portus::ipc::Ipc;
use portus::lang::Scope;
use portus::{Config, CongAlg, Datapath, DatapathInfo, DatapathTrait, Report};
use rand::{distributions::Uniform, thread_rng, Rng, ThreadRng};
use structopt::StructOpt;
use rustfft::FFTplanner;

#[derive(Clone, Copy, Debug)]
pub enum FlowMode {
    XTCP,
    Delay,
}

impl std::str::FromStr for FlowMode {
    type Err = failure::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "XTCP" => Ok(FlowMode::XTCP),
            "Delay" => Ok(FlowMode::Delay),
            _ => bail!("Unknown FlowMode {}", s),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum DelayMode {
    Copa,
    Nimbus,
    Vegas,
}

impl std::str::FromStr for DelayMode {
    type Err = failure::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "Copa" => Ok(DelayMode::Copa),
            "Nimbus" => Ok(DelayMode::Nimbus),
            "Vegas" => Ok(DelayMode::Vegas),
            _ => bail!("Unknown DelayMode {}", s),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum LossMode {
    Cubic,
    MulTCP,
    Bundle,
}

impl std::str::FromStr for LossMode {
    type Err = failure::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "Cubic" => Ok(LossMode::Cubic),
            "MulTCP" => Ok(LossMode::MulTCP),
            "Bundle" => Ok(LossMode::Bundle),
            _ => bail!("Unknown LossMode {}", s),
        }
    }
}

#[derive(StructOpt, Debug, Clone)]
#[structopt(name = "nimbus")]
pub struct NimbusConfig {
    #[structopt(long = "ipc", default_value = "unix")]
    pub ipc: String,

    #[structopt(long = "use_switching")]
    pub use_switching: bool,

    #[structopt(long = "pass_through")]
    pub pass_through: bool,

    #[structopt(long = "bw_est_mode")]
    pub bw_est_mode: bool,

    #[structopt(long = "use_ewma")]
    pub use_ewma: bool,

    #[structopt(long = "set_win_cap")]
    pub set_win_cap: bool,

    #[structopt(long = "delay_threshold", default_value = "1.25")]
    pub delay_threshold: f64,

    #[structopt(long = "init_delay_threshold", default_value = "1.25")]
    pub init_delay_threshold: f64,

    #[structopt(long = "pulse_size", default_value = "0.25")]
    pub pulse_size: f64,

    #[structopt(long = "frequency", default_value = "5.0")]
    pub frequency: f64,

    #[structopt(long = "switching_thresh", default_value = "0.4")]
    pub switching_thresh: f64,

    #[structopt(long = "uest", default_value = "12000000.0")]
    pub uest: f64,

    #[structopt(long = "bundler_qlen", default_value = "300")]
    pub bundler_qlen: usize,

    #[structopt(long = "bundler_qlen_alpha", default_value = "150.0")]
    pub bundler_qlen_alpha: f64,

    #[structopt(long = "bundler_qlen_beta", default_value = "15000.0")]
    pub bundler_qlen_beta: f64,

    #[structopt(long = "flow_mode", default_value = "XTCP")]
    pub flow_mode: FlowMode,

    #[structopt(long = "delay_mode", default_value = "Nimbus")]
    pub delay_mode: DelayMode,

    #[structopt(long = "loss_mode", default_value = "MulTCP")]
    pub loss_mode: LossMode,

    #[structopt(long = "xtcp_flows", default_value = "2")]
    pub xtcp_flows: usize,
}

pub struct Nimbus<T: Ipc> {
    control_channel: Datapath<T>,
    logger: Option<slog::Logger>,
    sc: Scope,
    sock_id: u32,
    mss: u32,

    rtt: time::Duration,
    ewma_rtt: f64,
    last_drop: Vec<time::Timespec>,
    last_update: time::Timespec,
    base_rtt: f64,
    wait_time: time::Duration,
    start_time: time::Timespec,

    uest: f64,
    alpha: f64,
    beta: f64,
    delay_threshold: f64,
    flow_mode: FlowMode,
    delay_mode: DelayMode,
    loss_mode: LossMode,
    rate: f64,
    ewma_rate: f64,
    cwnd: Vec<f64>,
    xtcp_flows: i32,
    ssthresh: Vec<f64>,
    cwnd_clamp: f64,

    init_delay_threshold: f64,
    frequency: f64,
    pulse_size: f64,
    zout_history: Vec<f64>,
    zt_history: Vec<f64>,
    rtt_history: Vec<f64>,
    measurement_interval: time::Duration,
    last_hist_update: time::Timespec,
    last_switch_time: time::Timespec,
    use_switching: bool,
    //switching_thresh: f64,
    ewma_elasticity: f64,
    ewma_master: f64,
    ewma_slave: f64,
    ewma_alpha: f64,

    //last_bw_est: time::Timespec,
    //index_bw_est:  Vec<i32>,
    rin_history: Vec<f64>,
    rout_history: Vec<f64>,
    agg_last_drop: time::Timespec,
    bw_est_mode: bool,
    max_rout: f64,
    ewma_rin: f64,
    ewma_rout: f64,
    use_ewma: bool,
    //set_win_cap: bool,
    master_mode: bool,
    switching_master: bool,
    r: ThreadRng,
    //last_slave_mode: time::Timespec,

    //cubic_init_cwnd: f64,
    cubic_cwnd: f64,
    cubic_ssthresh: f64,
    cwnd_cnt: f64,
    tcp_friendliness: bool,
    cubic_beta: f64,
    fast_convergence: bool,
    c: f64,
    wlast_max: f64,
    epoch_start: f64,
    origin_point: f64,
    d_min: f64,
    wtcp: f64,
    k: f64,
    ack_cnt: f64,
    cnt: f64,

    bundler_qlen_target: f64,
    bundler_qlen_alpha: f64,
    bundler_qlen_beta: f64,
    bundler_ewma_qlen: f64,
    bundler_last_qlen: f64,
    bundler_qlen_factor: f64,
    bundler_clamp_rate: f64,

    pkts_in_last_rtt: f64,
    velocity: f64,
    cur_direction: f64,
    prev_direction: f64,
    prev_update_rtt: time::Timespec,
    pass_through: bool,
}

impl<T: Ipc> Nimbus<T> {
    fn send_pattern(&self, mut rate: f64, _wait_time: time::Duration) {
        if (time::get_time() - self.start_time).num_seconds() < 1 {
            rate = 2_000_000.0;
        }

        let win = (self.mss as f64).max(rate * 2.0 * (self.rtt.num_milliseconds() as f64) * 0.001);
        self.control_channel
            .update_field(&self.sc, &[("Rate", rate as u32), ("Cwnd", win as u32)])
            .unwrap_or_else(|_| ());
    }

    fn install(&mut self, wait_time: time::Duration) -> Scope {
        self.control_channel
            .set_program(
                String::from("nimbus_program"),
                Some(&[("report_time", wait_time.num_microseconds().unwrap() as u32)][..]),
            )
            .unwrap()
    }

    fn get_fields(&mut self, m: &Report) -> Option<(u32, u32, f64, f64, u32, bool, u32)> {
        let sc = &self.sc;
        let acked = m
            .get_field("Report.acked", sc)
            .expect("expected acked field in returned measurement") as u32;
        let rtt = m
            .get_field("Report.rtt", sc)
            .expect("expected rtt field in returned measurement") as u32;
        let rin = m
            .get_field("Report.rin", sc)
            .expect("expected rin field in returned measurement") as f64;
        let rout = m
            .get_field("Report.rout", sc)
            .expect("expected rout field in returned measurement") as f64;
        let loss = m
            .get_field("Report.loss", sc)
            .expect("expected loss field in returned measurement") as u32;
        let was_timeout = m
            .get_field("Report.timeout", sc)
            .expect("expected timeout field in returned measurement")
            == 1;
        let qlen = m
            .get_field("Report.qlen", sc)
            .expect("expected qlen field in returned measurement") as u32;
        Some((acked, rtt, rin, rout, loss, was_timeout, qlen))
    }
}

impl<T: Ipc> CongAlg<T> for Nimbus<T> {
    type Config = NimbusConfig;

    fn name() -> String {
        String::from("nimbus")
    }

    fn init_programs(_cfg: Config<T, Self>) -> Vec<(String, String)> {
        vec![(
            String::from("nimbus_program"),
            String::from(
                "
                (def 
                    (Report
                        (volatile acked 0)
                        (volatile rtt 0)
                        (volatile loss 0)
                        (volatile rin 0)
                        (volatile rout 0)
                        (volatile timeout false)
                        (volatile qlen 0)
                    )
                    (report_time 0)
                )
                (when true
                    (:= Report.acked (+ Report.acked Ack.bytes_acked))
                    (:= Report.rtt Flow.rtt_sample_us)
                    (:= Report.rin Flow.rate_outgoing)
                    (:= Report.rout Flow.rate_incoming)
                    (:= Report.loss Ack.lost_pkts_sample)
                    (:= Report.timeout Flow.was_timeout)
                    (:= Report.qlen Flow.bytes_pending)
                    (fallthrough)
                )
                (when (|| Report.timeout (> Report.loss 0))
                    (report)
                    (:= Micros 0)
                )
                (when (> Micros report_time)
                    (report)
                    (:= Micros 0)
                )
            ",
            ),
        )]
    }

    fn create(control: Datapath<T>, cfg: Config<T, Nimbus<T>>, info: DatapathInfo) -> Self {
        cfg.logger.as_ref().map(|log| {
            info!(log, "[nimbus] starting";
                "ipc" => ?cfg.config.ipc,
                "use_switching" => ?cfg.config.use_switching,
                "bw_est_mode" => ?cfg.config.bw_est_mode ,
                "use_ewma" => ?cfg.config.use_ewma ,
                "set_win_cap" => ?cfg.config.set_win_cap ,
                "delay_threshold" => ?cfg.config.delay_threshold ,
                "init_delay_threshold" => ?cfg.config.init_delay_threshold ,
                "pulse_size" => ?cfg.config.pulse_size ,
                "frequency" => ?cfg.config.frequency ,
                "switching_thresh" => ?cfg.config.switching_thresh,
                "uest" => ?cfg.config.uest ,
                "flow_mode" => ?cfg.config.flow_mode ,
                "delay_mode" => ?cfg.config.delay_mode ,
                "loss_mode" => ?cfg.config.loss_mode,
                "xtcp_flows" => ?cfg.config.xtcp_flows,
                "pass_through" => ?cfg.config.pass_through,
            );
        });

        let mut s = Self {
            sock_id: info.sock_id,
            control_channel: control,
            sc: Default::default(),
            logger: cfg.logger,
            mss: info.mss,

            use_switching: cfg.config.use_switching,
            bw_est_mode: cfg.config.bw_est_mode, // default to true
            delay_threshold: cfg.config.delay_threshold,
            xtcp_flows: cfg.config.xtcp_flows as i32,
            init_delay_threshold: cfg.config.init_delay_threshold,
            frequency: cfg.config.frequency,
            pulse_size: cfg.config.pulse_size,
            //switching_thresh: cfg.config.switching_thresh,
            flow_mode: cfg.config.flow_mode,
            delay_mode: cfg.config.delay_mode,
            loss_mode: cfg.config.loss_mode,
            uest: cfg.config.uest,
            use_ewma: cfg.config.use_ewma,
            pass_through: cfg.config.pass_through,
            //set_win_cap:  cfg.config.set_win_cap_arg,
            base_rtt: -0.001f64, // careful
            last_drop: vec![],
            last_update: time::get_time(),
            rtt: time::Duration::milliseconds(300),
            ewma_rtt: 0.1f64,
            start_time: time::Timespec::new(0, 0),
            ssthresh: vec![],
            cwnd_clamp: 2e6 * 1448.0,

            alpha: 0.8f64,
            beta: 0.5f64,
            rate: 100000f64,
            ewma_rate: 10000f64,
            cwnd: vec![],

            zout_history: vec![],
            zt_history: vec![],
            rtt_history: vec![],
            measurement_interval: time::Duration::milliseconds(10),
            last_hist_update: time::get_time(),
            last_switch_time: time::get_time(),
            ewma_elasticity: 1.0f64,
            ewma_slave: 1.0f64,
            ewma_master: 1.0f64,
            ewma_alpha: 0.01f64,

            //last_bw_est: time::get_time(),
            //index_bw_est: vec![],
            rin_history: vec![],
            rout_history: vec![],
            agg_last_drop: time::get_time(),
            max_rout: 0.0f64,
            ewma_rin: 0.0f64,
            ewma_rout: 0.0f64,

            wait_time: time::Duration::milliseconds(5),

            master_mode: true,
            switching_master: false,
            r: thread_rng(),
            //last_slave_mode: time::get_time(),

            //cubic_init_cwnd: 10f64,
            cubic_cwnd: 10f64,
            cubic_ssthresh: ((0x7fffffff as f64) / 1448.0),
            cwnd_cnt: 0f64,
            tcp_friendliness: true,
            cubic_beta: 0.3f64,
            fast_convergence: true,
            c: 0.4f64,

            pkts_in_last_rtt: 0f64,
            velocity: 1f64,
            cur_direction: 0f64,
            prev_direction: 0f64,
            prev_update_rtt: time::get_time(),

            wlast_max: 0f64,
            epoch_start: -0.0001f64,
            origin_point: 0f64,
            d_min: -0.0001f64,
            wtcp: 0f64,
            k: 0f64,
            ack_cnt: 0f64,
            cnt: 0f64,

            bundler_qlen_target: cfg.config.bundler_qlen as f64,
            bundler_qlen_alpha: cfg.config.bundler_qlen_alpha,
            bundler_qlen_beta: cfg.config.bundler_qlen_beta,
            bundler_ewma_qlen: 0.,
            bundler_last_qlen: 0.,
            bundler_qlen_factor: 1.0,
            bundler_clamp_rate: 1e5,
        };

        s.cwnd = (0..s.xtcp_flows)
            .map(|_| s.rate / (s.xtcp_flows as f64))
            .collect();
        s.last_drop = (0..s.xtcp_flows).map(|_| time::get_time()).collect();
        s.ssthresh = (0..s.xtcp_flows).map(|_| s.cwnd_clamp).collect();

        //s.cubic_reset(); Careful
        let wt = s.wait_time;
        s.sc = s.install(wt);
        s.send_pattern(s.rate, wt);

        s
    }

    fn on_report(&mut self, _sock_id: u32, m: Report) {
        let (acked, rtt_us, mut rin, mut rout, loss, was_timeout, qlen) =
            self.get_fields(&m).unwrap();

        self.rtt = time::Duration::microseconds(rtt_us as i64);

        if loss > 0 {
            self.handle_drop();
            return;
        }

        if was_timeout {
            self.handle_timeout(); // Careful
            return;
        }

        let rtt_seconds = rtt_us as f64 * 1e-6;
        self.ewma_rtt = 0.95 * self.ewma_rtt + 0.05 * rtt_seconds;
        if self.base_rtt <= 0.0 || rtt_seconds < self.base_rtt {
            // careful
            self.base_rtt = rtt_seconds;
        }

        self.pkts_in_last_rtt = acked as f64 / self.mss as f64;
        if self.start_time.sec == 0 {
            self.start_time = time::get_time();
        }
        let now = time::get_time();
        let elapsed = (now - self.start_time).num_milliseconds() as f64 * 1e-3;
        //let mut  float_rin = rin as f64;
        //let mut float_rout = rout as f64; // careful

        self.ewma_rin = 0.2 * rin + 0.8 * self.ewma_rin;
        //if self.ewma_rout == 0.0 {
        //    self.ewma_rout = rout;
        //}

        self.ewma_rout = 0.02 * rout + 0.98 * self.ewma_rout;

        if self.use_ewma {
            rin = self.ewma_rin;
            rout = self.ewma_rout;
        }

        if self.max_rout < self.ewma_rout {
            self.max_rout = self.ewma_rout;
            if self.bw_est_mode {
                self.uest = self.max_rout;
            }
        }

        let mut zt = self.uest * (rin / rout) - rin;
        if zt.is_nan() {
            zt = 0.0;
        }

        while now > self.last_hist_update {
            self.rin_history.push(rin);
            self.rout_history.push(rout);
            self.zout_history.push(self.uest - rout);
            self.zt_history.push(zt);
            self.rtt_history
                .push(self.rtt.num_milliseconds() as f64 * 1e-3);
            self.last_hist_update = self.last_hist_update + self.measurement_interval;
        }

        match self.flow_mode {
            FlowMode::Delay => {
                self.frequency = 6.0f64;
                self.update_rate_delay(rin, zt, acked as u64);
            }
            FlowMode::XTCP => {
                self.frequency = 5.0f64;
                self.update_rate_loss(acked as u64, qlen);
            }
        }

        self.rate = self.rate.max(0.05 * self.uest);

        // NOTE: this was previously below pulse creation, which was overwriting the pulses
        // controller to keep some queue at inbox
        // Moving inside of update_rate_loss
        //if let FlowMode::XTCP = self.flow_mode {
        //    self.control_inbox_queue(qlen);
        //}

        if self.master_mode {
            self.rate = self.elasticity_est_pulse().max(0.05 * self.uest);
        }

        self.rate = self.rate.max(0.05 * self.uest);

        self.send_pattern(self.rate, self.wait_time);
        self.should_switch_flow_mode();
        self.last_update = time::get_time();

        self.logger.as_ref().map(|log| {
            debug!(log, "[nimbus] got ack";
                "ID" => self.sock_id,
                "base_rtt" => self.base_rtt,
                "curr_rate" => self.rate * 8.0,
                "curr_cwnd" => self.cwnd[0],
                "newly_acked" => acked,
                "rin" => rin * 8.0,
                "rout" => rout * 8.0,
                "ewma_rin" => self.ewma_rin * 8.0,
                "ewma_rout" => self.ewma_rout * 8.0,
                "max_ewma_rout" => self.max_rout * 8.0,
                "zt" => zt * 8.0,
                "rtt" => rtt_seconds,
                "uest" => self.uest * 8.0,
                "qlen" => qlen, //self.bundler_ewma_qlen,
                "qlen_factor" => self.bundler_qlen_factor,
                "elapsed" => elapsed,
            );
        });
        //n.last_ack = m.Ack Careful
    }
}

impl<T: Ipc> Nimbus<T> {
    fn control_inbox_queue(&mut self, qlen: u32) {
        if self.pass_through {
            self.rate = 1.25 * self.ewma_rout;
            return;
        }

        let qlen = qlen as f64;

        //self.bundler_ewma_qlen = 0.98 * self.bundler_ewma_qlen + 0.02 * qlen;
        //let qlen = self.bundler_ewma_qlen;

        // beta / alpha == 1 / (update interval)
        let alpha = self.bundler_qlen_alpha;
        let beta = self.bundler_qlen_beta;

        // adjusted target
        //
        //let elapsed = (time::get_time() - self.start_time).num_milliseconds() as f64 * 0.001;
        //let mut phase = elapsed * self.frequency;
        //phase -= phase.floor();
        //let adj_target = if phase < 0.25 {
        //    // self.bundler_qlen_target - integrate mu / 4 sin(t pi / 50ms) from 0 to t
        //    // self.bundler_qlen_target - mu / 4 * 50ms / pi * 1pkt/1500Bytes * (1 - cos(t pi/50ms))
        //    let t = phase * 200.; //ms
        //    self.bundler_qlen_target
        //        - (self.uest / 4.)
        //            * (50e-3 / std::f64::consts::PI)
        //            * (1. / 1500.)
        //            * (1. - ((t * std::f64::consts::PI / 50.).cos()))
        //} else {
        //    // self.bundler_qlen_target
        //    //  - integrate mu / 4 sin(t pi / 50ms) from 0 to 50ms (above)
        //    //  + integrate mu / 12 sin(t pi / 150ms) from 0 to 150ms)
        //    let t = (phase - 0.25) * 200.;
        //    let up_pulse_int =
        //        (self.uest / 4.) * (50e-3 / std::f64::consts::PI) * (1. / 1500.) * (2.);
        //    self.bundler_qlen_target - up_pulse_int
        //        + (self.uest / 12.)
        //            * (150e-3 / std::f64::consts::PI)
        //            * (1. / 1500.)
        //            * (1. - ((t * std::f64::consts::PI / 150.).cos()))
        //};
        //
        // without adjusted target
        let adj_target = self.bundler_qlen_target;

        self.bundler_qlen_factor = adj_target;

        self.bundler_clamp_rate = self.bundler_clamp_rate
            + alpha * (qlen - adj_target)
            + beta * ((qlen - adj_target) - self.bundler_last_qlen);

        // with ewma
        // self.rate = 0.98 * self.rate + 0.02 * self.bundler_clamp_rate;
        // without ewma
        self.rate = self.bundler_clamp_rate;

        self.bundler_last_qlen = qlen - adj_target;
    }

    fn handle_drop(&mut self) {
        match self.loss_mode {
            LossMode::Cubic => self.cubic_drop(),
            LossMode::MulTCP => self.mul_tcp_drop(),
            LossMode::Bundle => (), // do nothing
        }
    }

    fn cubic_drop(&mut self) {
        if (time::get_time() - self.last_drop[0]) < self.rtt {
            return;
        }
        self.epoch_start = -0.0001f64; //careful
        if (self.cubic_cwnd < self.wlast_max) && self.fast_convergence {
            self.wlast_max = self.cubic_cwnd * ((2.0 - self.cubic_beta) / 2.0);
        } else {
            self.wlast_max = self.cubic_cwnd;
        }
        self.cubic_cwnd = self.cubic_cwnd * (1.0 - self.cubic_beta);
        self.cubic_ssthresh = self.cubic_cwnd;
        self.cwnd[0] = self.cubic_cwnd * 1448.0;
        self.rate = self.cwnd[0] / (self.rtt.num_milliseconds() as f64 * 0.001);
        match self.flow_mode {
            FlowMode::XTCP => self.send_pattern(self.rate, self.wait_time),
            _ => (),
        };

        self.logger.as_ref().map(|log| {
            debug!(log, "[nimbus cubic] got drop"; 
                "ID" => self.sock_id as u32,
                "time since last drop" => (time::get_time() - self.last_drop[0]).num_milliseconds() as f64 * 0.001,
                "rtt" => self.rtt.num_milliseconds() as f64 * 0.001,
            );
        });
        self.last_drop[0] = time::get_time();
        self.agg_last_drop = time::get_time();
    }

    fn mul_tcp_drop(&mut self) {
        let total_cwnd: f64 = self.cwnd.iter().sum();

        let mut rng = thread_rng();
        if total_cwnd as u64 == 0 {
            return;
        }

        let bounds = Uniform::new(0, total_cwnd as u64);
        let j: u64 = rng.sample(bounds);

        let i = self
            .cwnd
            .iter()
            .scan(0, |cum, &x| {
                *cum += x as u64;
                Some(*cum)
            })
            .position(|x| x > j)
            .unwrap_or_else(|| self.xtcp_flows as usize - 1);

        if (time::get_time() - self.last_drop[i]).num_milliseconds() as f64 * 0.001 < self.base_rtt
        {
            return;
        }

        self.cwnd[i as usize] /= 2.0;
        self.update_rate_mul_tcp(0);
        // not perfect
        self.ssthresh[i as usize] = self.cwnd[i as usize];
        self.rate = self.rate.max(0.05 * self.uest);
        match self.flow_mode {
            FlowMode::XTCP => self.send_pattern(self.rate, self.wait_time),
            _ => (),
        };

        //if len(n.index_bw_est) > 1 && float64((len(n.rin_history)-n.index_bw_est[len(n.index_bw_est)-1]))*n.measurement_interval.Seconds() < 2*n.rtt.Seconds() {
        //  n.index_bw_est = n.index_bw_est[:len(n.index_bw_est)-1]
        //} // Careful

        self.logger.as_ref().map(|log| {
            debug!(log, "[nimbus XTCP] got drop"; 
                "ID" => self.sock_id as u32,
                "time since last drop" => (time::get_time()-self.last_drop[0]).num_milliseconds() as f64 * 0.001,
                "rtt" => self.rtt.num_milliseconds() as f64 * 0.001,
                "xtcflows" => i,
            );
        });

        self.last_drop[i as usize] = time::get_time();
        self.agg_last_drop = time::get_time();
    }

    fn update_rate_delay(&mut self, rin: f64, zt: f64, new_bytes_acked: u64) {
        let curr_rtt = self.rtt.num_milliseconds() as f64 * 0.001;
        match self.delay_mode {
            DelayMode::Vegas => {
                let mut total_cwnd = 0.0;
                for i in 0..self.xtcp_flows {
                    total_cwnd += self.cwnd[i as usize];
                }

                if self.ewma_rtt < (1.0 * self.base_rtt + 0.15) {
                    self.cwnd[0] += self.mss as f64 * (new_bytes_acked as f64 / total_cwnd);
                } else {
                    self.cwnd[0] -= self.mss as f64 * (new_bytes_acked as f64 / total_cwnd);
                }

                total_cwnd = 0.0;
                for i in 0..self.xtcp_flows {
                    total_cwnd += self.cwnd[i as usize];
                }
                if self.master_mode {
                    self.rate = total_cwnd / curr_rtt;
                } else {
                    //n.last_slave_mode = time.Now()
                    //n.rate = total_cwnd / n.rtt.Seconds()
                    self.rate = total_cwnd / self.ewma_rtt;
                }
            }
            DelayMode::Copa => {
                let mut increase = false;
                if (curr_rtt * self.mss as f64)
                    > ((curr_rtt - 1.2 * self.base_rtt) * (1.9 / 2.0) * self.cwnd[0])
                {
                    increase = true;
                    self.cur_direction += 1.0;
                } else {
                    self.cur_direction -= 1.0;
                }
                if (time::get_time() - self.prev_update_rtt) > self.rtt {
                    if (self.prev_direction > 0.0 && self.cur_direction > 0.0)
                        || (self.prev_direction < 0.0 && self.cur_direction < 0.0)
                    {
                        self.velocity *= 2.0;
                    } else {
                        self.velocity = 1.0;
                    }
                    if self.velocity > 32.0 {
                        // TODO somewhere around 50
                        self.velocity = 32.0;
                    }
                    self.prev_direction = self.cur_direction;
                    self.cur_direction = 0.0;
                    self.prev_update_rtt = time::get_time();
                }
                let change = (self.velocity * self.mss as f64 * new_bytes_acked as f64)
                    / (self.cwnd[0] * (1.0 / 2.0));

                if increase {
                    self.cwnd[0] += change;
                } else {
                    if change + 15000.0 > self.cwnd[0] {
                        self.cwnd[0] = 15000.0;
                    } else {
                        self.cwnd[0] -= change;
                    }
                }
                self.rate = self.cwnd[0] / curr_rtt;
            }
            DelayMode::Nimbus => {
                let delta = curr_rtt;
                self.rate = rin + self.alpha * (self.uest - zt - rin)
                    - ((self.uest * self.beta) / delta)
                        * (curr_rtt - (self.delay_threshold * self.base_rtt));
                if self.delay_threshold > self.init_delay_threshold {
                    self.delay_threshold -=
                        ((self.measurement_interval.num_milliseconds() as f64 * 0.001) / 0.1)
                            * 0.05;
                }
            }
        }
    }

    //fn cubic_reset(&mut self) {
    //    self.wlast_max = 0.0;
    //    self.epoch_start = -0.0001;
    //    self.origin_point = 0.0;
    //    self.d_min = -0.0001; //careful
    //    self.wtcp = 0.0;
    //    self.k = 0.0;
    //    self.ack_cnt = 0.0;
    //}

    fn update_rate_loss(&mut self, new_bytes_acked: u64, qlen: u32) {
        match self.loss_mode {
            LossMode::Cubic => self.update_rate_cubic(new_bytes_acked),
            LossMode::MulTCP => self.update_rate_mul_tcp(new_bytes_acked),
            LossMode::Bundle => self.control_inbox_queue(qlen),
            //{
            // the offered load will naturally match the fair share
            // so just get out of the way
            // pulsing is still needed to cut delays when appropriate
            //

            //self.rate = 1.25 * self.ewma_rout;
            //self.rate = self.bundler_clamp_rate;
            //self.rate = self.ewma_rout + self.uest / 4.;
            //self.rate = self.uest / 2.;
            //self.ewma_rate = self.rate;
            //}
        }
    }

    fn update_rate_cubic(&mut self, new_bytes_acked: u64) {
        let mut no_of_acks = (new_bytes_acked as f64) / self.mss as f64;
        if self.cubic_cwnd < self.cubic_ssthresh {
            if (self.cubic_cwnd + no_of_acks) < self.cubic_ssthresh {
                self.cubic_cwnd += no_of_acks;
                no_of_acks = 0.0;
            } else {
                no_of_acks -= self.cubic_ssthresh - self.cubic_cwnd;
                self.cubic_cwnd = self.cubic_ssthresh;
            }
        }
        let rtt_seconds = self.rtt.num_milliseconds() as f64 * 0.001;
        for _ in 0..no_of_acks as usize {
            if self.d_min <= 0.0 || rtt_seconds < self.d_min {
                self.d_min = rtt_seconds;
            }
            self.cubic_update();
            if self.cwnd_cnt > self.cnt {
                self.cubic_cwnd = self.cubic_cwnd + 1.0;
                self.cwnd_cnt = 0.0;
            } else {
                self.cwnd_cnt = self.cwnd_cnt + 1.0;
            }
        }
        self.cwnd[0] = self.cubic_cwnd * 1448.0;
        let total_cwnd = self.cwnd[0];
        if self.master_mode {
            self.rate = total_cwnd / rtt_seconds;
        } else {
            self.rate = total_cwnd / self.ewma_rtt;
        }
        self.ewma_rate = self.rate;
    }

    fn cubic_update(&mut self) {
        self.ack_cnt = self.ack_cnt + 1.0;
        if self.epoch_start <= 0.0 {
            self.epoch_start =
                (time::get_time().sec as f64) + f64::from(time::get_time().nsec) / 1e9;
            if self.cubic_cwnd < self.wlast_max {
                self.k = (0.0f64.max((self.wlast_max - self.cubic_cwnd) / self.c)).powf(1.0 / 3.0);
                self.origin_point = self.wlast_max;
            } else {
                self.k = 0.0;
                self.origin_point = self.cubic_cwnd;
            }
            self.ack_cnt = 1.0;
            self.wtcp = self.cubic_cwnd;
        }
        let t = (time::get_time().sec as f64) + f64::from(time::get_time().nsec) / 1e9 + self.d_min
            - self.epoch_start;
        let target = self.origin_point + self.c * ((t - self.k) * (t - self.k) * (t - self.k));
        if target > self.cubic_cwnd {
            self.cnt = self.cubic_cwnd / (target - self.cubic_cwnd);
        } else {
            self.cnt = 100.0 * self.cubic_cwnd;
        }
        if self.tcp_friendliness {
            self.cubic_tcp_friendliness();
        }
    }

    fn cubic_tcp_friendliness(&mut self) {
        self.wtcp = self.wtcp
            + (((3.0 * self.cubic_beta) / (2.0 - self.cubic_beta))
                * (self.ack_cnt / self.cubic_cwnd));
        self.ack_cnt = 0.0;
        if self.wtcp > self.cubic_cwnd {
            let max_cnt = self.cubic_cwnd / (self.wtcp - self.cubic_cwnd);
            if self.cnt > max_cnt {
                self.cnt = max_cnt;
            }
        }
    }

    fn update_rate_mul_tcp(&mut self, new_bytes_acked: u64) {
        let total_cwnd: f64 = self.cwnd.iter().sum();

        for i in 0..self.xtcp_flows {
            let mut xtcp_new_byte_acked =
                (new_bytes_acked as f64) * (self.cwnd[i as usize] / total_cwnd);
            if self.cwnd[i as usize] < self.ssthresh[i as usize] {
                if self.cwnd[i as usize] + xtcp_new_byte_acked > self.ssthresh[i as usize] {
                    xtcp_new_byte_acked -= self.ssthresh[i as usize] - self.cwnd[i as usize];
                    self.cwnd[i as usize] = self.ssthresh[i as usize];
                } else {
                    self.cwnd[i as usize] += xtcp_new_byte_acked;
                    xtcp_new_byte_acked = 0.0;
                }
            }

            self.cwnd[i as usize] +=
                self.mss as f64 * (xtcp_new_byte_acked / self.cwnd[i as usize]);
            self.cwnd[i as usize] = self.cwnd[i as usize].min(self.cwnd_clamp);
        }

        if self.master_mode {
            self.rate = total_cwnd / (self.rtt.num_milliseconds() as f64 * 0.001);
        } else {
            self.rate = total_cwnd / self.ewma_rtt;
        }

        self.ewma_rate = self.rate
    }

    fn elasticity_est_pulse(&mut self) -> f64 {
        //return self.rate; // turn off pulsing

        let elapsed = (time::get_time() - self.start_time).num_milliseconds() as f64 * 0.001;
        let fr_modified = self.uest;
        let mut phase = elapsed * self.frequency;
        phase -= phase.floor();
        let up_ratio = 0.25;
        if phase < up_ratio {
            return self.rate
                + self.pulse_size
                    * fr_modified
                    * (2.0 * std::f64::consts::PI * phase * (0.5 / up_ratio)).sin();
        } else {
            let r = self.rate
                + (up_ratio / (1.0 - up_ratio))
                    * self.pulse_size
                    * fr_modified
                    * (2.0
                        * std::f64::consts::PI
                        * (0.5 + (phase - up_ratio) * (0.5 / (1.0 - up_ratio))))
                        .sin();
            return r;
        }
    }

    fn switch_to_delay(&mut self, rtt: time::Duration) {
        if !self.use_switching {
            return;
        }

        match self.flow_mode {
            FlowMode::Delay => return,
            _ if (time::get_time() - self.last_switch_time).num_seconds() < 5 => return,
            _ => (),
        };

        self.delay_threshold = self
            .init_delay_threshold
            .max((rtt.num_milliseconds() as f64 * 0.001) / self.base_rtt);

        self.logger.as_ref().map(|log| {
            debug!(log, "switched mode";
                "ID" => self.sock_id,
                "elapsed" => (time::get_time() - self.start_time).num_milliseconds() as f64 * 0.001,
                "from" =>  ?self.flow_mode,
                "to" => "DELAY",
                "Delay_theshold" => self.delay_threshold,
            );
        });

        self.flow_mode = FlowMode::Delay;
        self.last_switch_time = time::get_time();
        self.velocity = 1.0;
        self.cur_direction = 0.0;
        self.prev_direction = 0.0;
        //TODO WHY IS THIS HERE
        //self.update_rate_loss(0, 0);
    }

    fn switch_to_xtcp(&mut self, _rtt: time::Duration) {
        if !self.use_switching {
            return;
        }

        match self.flow_mode {
            FlowMode::XTCP => return,
            _ => (),
        };

        self.logger.as_ref().map(|log| {
            debug!(log, "switched mode";
                "ID" => self.sock_id,
                "elapsed" => (time::get_time() - self.start_time).num_milliseconds() as f64 * 0.001,
                "from" =>  ?self.flow_mode,
                "to" => "XTCP",
            );
        });

        self.flow_mode = FlowMode::XTCP;
        self.rate = self.rout_history[self.rout_history.len()
            - ((5.0 / (self.measurement_interval.num_milliseconds() as f64 * 0.001)) as usize)];

        match self.loss_mode {
            LossMode::Cubic => {
                self.epoch_start = -0.0001; // careful
                self.cwnd[0] = self.rate * self.rtt.num_milliseconds() as f64 * 0.001;
                self.cubic_cwnd = self.cwnd[0] / self.mss as f64;
                self.cubic_ssthresh = self.cubic_cwnd;
                self.k = 0.0;
                self.origin_point = self.cubic_cwnd;
            }
            LossMode::MulTCP => {
                for i in 0..self.xtcp_flows {
                    self.cwnd[i as usize] = self.rate
                        * (self.rtt.num_milliseconds() as f64 * 0.001)
                        / (self.xtcp_flows as f64);
                    self.ssthresh[i as usize] = self.cwnd[i as usize];
                }
            }
            LossMode::Bundle => {
                self.bundler_clamp_rate = self.rate;
            }
        };

        self.last_switch_time = time::get_time();
    }

    fn should_switch_flow_mode(&mut self) {
        let mut duration_of_fft = if self.master_mode { 5.0 } else { 2.5 };
        let t = self.measurement_interval.num_milliseconds() as f64 * 0.001;

        // get next higher power of 2
        let n = (duration_of_fft / t) as i32;
        let n = if n.count_ones() != 1 {
            1 << (32 - n.leading_zeros())
        } else {
            n
        };

        duration_of_fft = (n as f64) * t;

        if (time::get_time() - self.start_time).num_seconds() < 10 {
            return;
        }

        let end_index = self.zt_history.len() - 1;
        let start_index = self.zt_history.len() - ((duration_of_fft + 1.0) / t) as usize;

        let raw_zt = &self.zt_history.clone()[start_index..end_index]; // careful: complexity
        let raw_rtt = &self.rtt_history.clone()[start_index..end_index];
        let raw_zout = &self.zout_history.clone()[start_index..end_index];

        let mut clean_zt: Vec<Complex<f64>> = Vec::new(); // careful: complexity
        let mut clean_zout: Vec<Complex<f64>> = Vec::new();
        let mut clean_rtt: Vec<Complex<f64>> = Vec::new();

        for i in 0..n {
            if i as usize >= raw_rtt.len() {
                return;
            }

            let j = i as usize + 2 * ((raw_rtt[i as usize] / t) as usize);
            if j >= raw_zt.len() {
                return;
            }

            clean_zt.push(Complex::new(raw_zt[j], 0.0));
            clean_zout.push(Complex::new(raw_zout[i as usize], 0.0));
            clean_rtt.push(Complex::new(raw_rtt[i as usize], 0.0));
        }

        let avg_rtt = time::Duration::milliseconds(
            (1e3 * self.mean_complex(&clean_rtt[(0.75 * (clean_rtt.len() as f32)) as usize..]))
                as i64,
        );
        let avg_zt = self.mean_complex(&clean_zt[(0.75 * (clean_zt.len() as f32)) as usize..]);

        clean_zt = self.detrend(clean_zt);
        clean_zout = self.detrend(clean_zout);

        //let mut fft_zt_temp = FFT::new(clean_zt.len(), false);
        let mut fft_zt_temp_plan = FFTplanner::new(false);
        let fft_zt_temp = fft_zt_temp_plan.plan_fft(clean_zt.len());
        let mut fft_zt = clean_zt.clone();
        fft_zt_temp.process(&mut clean_zt[..], &mut fft_zt[..]);

        //let mut fft_zout_temp = FFT::new(clean_zout.len(), false);
        let mut fft_zout_temp_plan = FFTplanner::new(false);
        let fft_zout_temp = fft_zout_temp_plan.plan_fft(clean_zout.len());
        let mut fft_zout = clean_zout.clone();
        fft_zout_temp.process(&mut clean_zout[..], &mut fft_zout[..]);

        let mut freq: Vec<f64> = Vec::new();
        for i in 0..((n / 2) as usize) {
            freq.push(i as f64 * (1.0 / (n as f64 * t)));
        }

        let expected_peak = self.frequency;
        let expected_peak2 = match self.flow_mode {
            FlowMode::Delay => 5.0,
            FlowMode::XTCP => 6.0,
        };

        if self.master_mode {
            if avg_zt < 0.1 * self.uest {
                self.ewma_elasticity = 0.0;
            } else if avg_zt > 0.9 * self.uest {
                self.ewma_elasticity =
                    (1.0 - self.ewma_alpha) * self.ewma_elasticity + self.ewma_alpha * 6.0;
            }

            let (_, mean_zt) = self.find_peak(
                2.2 * expected_peak,
                3.8 * expected_peak,
                &freq[..],
                &fft_zt[..],
            );
            let (exp_peak_zt, _) = self.find_peak(
                expected_peak - 0.5,
                expected_peak + 0.5,
                &freq[..],
                &fft_zt[..],
            );
            let (exp_peak_zout, _) = self.find_peak(
                expected_peak - 0.5,
                expected_peak + 0.5,
                &freq[..],
                &fft_zout[..],
            );
            let (other_peak_zt, _) = self.find_peak(
                expected_peak + 1.5,
                2.0 * expected_peak - 0.5,
                &freq[..],
                &fft_zt[..],
            );
            let (other_peak_zout, _) = self.find_peak(
                expected_peak + 1.5,
                2.0 * expected_peak - 0.5,
                &freq[..],
                &fft_zout[..],
            );
            let mut elasticity2 = fft_zt[exp_peak_zt].norm() / fft_zt[other_peak_zt].norm();
            let elasticity =
                (fft_zt[exp_peak_zt].norm() - mean_zt) / fft_zout[exp_peak_zout].norm();
            if fft_zt[exp_peak_zt].norm() < 0.25 * fft_zout[exp_peak_zout].norm() {
                elasticity2 = elasticity2.min(3.0);
                elasticity2 *=
                    ((fft_zt[exp_peak_zt].norm() / fft_zout[exp_peak_zout].norm()) / 0.25).min(1.0);
            }
            self.ewma_elasticity =
                (1.0 - self.ewma_alpha) * self.ewma_elasticity + self.ewma_alpha * elasticity2;

            if (fft_zout[exp_peak_zout].norm() / fft_zout[other_peak_zout].norm()) < 2.0 {
                self.ewma_elasticity =
                    (1.0 - self.ewma_alpha) * self.ewma_elasticity + self.ewma_alpha * 3.0;
            }

            let (exp_peak_zt_master, _) = self.find_peak(4.5, 6.5, &freq[..], &fft_zt[..]);
            let (exp_peak_zout_master, _) = self.find_peak(4.5, 6.5, &freq[..], &fft_zout[..]);
            self.ewma_master = (1.0 - 2.0 * self.ewma_alpha) * self.ewma_master
                + 2.0
                    * self.ewma_alpha
                    * (fft_zt[exp_peak_zt_master].norm() / fft_zout[exp_peak_zout_master].norm());

            if (time::get_time() - self.start_time).num_seconds() < 15 {
                return;
            }

            if self.ewma_elasticity > 2.25 {
                self.switch_to_xtcp(avg_rtt);
            } else if self.ewma_elasticity < 2.0 {
                self.switch_to_delay(avg_rtt);
            }

            if self.ewma_master > 2.0 {
                self.switch_to_slave();
            }

            self.logger.as_ref().map(|log| {
                debug!(log, "elasticity_inf";
                    "ID" => self.sock_id,
                    "Zout_peak_val" => fft_zout[exp_peak_zout].norm(),
                    "Zt_peak_val" => fft_zt[exp_peak_zt].norm(),
                    "elapsed" => (time::get_time() - self.start_time).num_seconds(),
                    "Elasticity" => elasticity,
                    "Elasticity2" => elasticity2,
                    "EWMAElasticity" => self.ewma_elasticity,
                    "EWMAMaster" => self.ewma_master,
                    "Expected Peak" => expected_peak,
                );
            });
        } else {
            let (exp_peak_zout, _) = self.find_peak(
                expected_peak - 0.5,
                expected_peak + 0.5,
                &freq[..],
                &fft_zout[..],
            );
            let (exp_peak_zout2, _) = self.find_peak(
                expected_peak2 - 0.5,
                expected_peak2 + 0.5,
                &freq[..],
                &fft_zout[..],
            );
            self.ewma_slave = (1.0 - 2.0 * self.ewma_alpha) * self.ewma_slave
                + 2.0
                    * self.ewma_alpha
                    * (fft_zout[exp_peak_zout2].norm() / fft_zout[exp_peak_zout].norm());

            let (exp_peak_zout_slave, _) = self.find_peak(4.5, 6.5, &freq[..], &fft_zout[..]);
            let (other_peak_zout_slave, _) = self.find_peak(7.0, 15.0, &freq[..], &fft_zout[..]);
            self.ewma_elasticity = (1.0 - self.ewma_alpha) * self.ewma_elasticity
                + self.ewma_alpha
                    * (fft_zout[exp_peak_zout_slave].norm()
                        / fft_zout[other_peak_zout_slave].norm());

            if (time::get_time() - self.start_time).num_seconds() < 15 {
                return;
            }

            if self.ewma_slave > 1.25 {
                self.ewma_slave = 0.0;
                match self.flow_mode {
                    FlowMode::Delay if (avg_zt > 0.1 * self.uest) => {
                        self.switch_to_xtcp(avg_rtt);
                    }
                    _ => self.switch_to_delay(avg_rtt),
                }
            }

            if self.ewma_elasticity < 1.5 {
                self.switch_to_master()
            }

            self.logger.as_ref().map(|log| {
                debug!(log, "elasticity_inf";
                    "ID" => self.sock_id,
                    "Zout_peak_val" => fft_zout[exp_peak_zout].norm(),
                    "Zout2Peak_val" => fft_zout[exp_peak_zout2].norm(),
                    "elapsed" => (time::get_time() - self.start_time).num_seconds(),
                    "EWMAElasticity" => self.ewma_elasticity,
                    "EWMASlave" => self.ewma_slave,
                    "Expected Peak" => expected_peak,
                    "Expected Peak2" => expected_peak2,
                );
            });
        }
    }

    fn switch_to_master(&mut self) {
        if !self.switching_master {
            return;
        }

        if self.r.gen::<f64>() < (0.005 * (self.ewma_rin / self.uest)) {
            self.logger.as_ref().map(|log| {
                debug!(log, "Switch To Master";
                    "ID" => self.sock_id,
                    "EWMAElasticity" => self.ewma_elasticity,
                    "elapsed" => (time::get_time() - self.start_time).num_seconds(),
                    "EWMASlave" => self.ewma_slave,
                );
            });

            self.master_mode = true;
            //n.ewma_elasticity = 3.0
            self.ewma_master = 1.0;
        }
    }

    fn switch_to_slave(&mut self) {
        if !self.switching_master {
            return;
        }

        if self.r.gen::<f64>() < 0.005 {
            self.logger.as_ref().map(|log| {
                debug!(log, "Switch To Slave";
                    "ID" => self.sock_id,
                    "EWMAElasticity" => self.ewma_elasticity,
                    "EWMAMaster" => self.ewma_master,
                    "elapsed" => (time::get_time() - self.start_time).num_seconds(),
                );
            });
            self.ewma_slave = 0.0;
            self.master_mode = false;
            //n.ewma_elasticity = 3.0
        }
    }

    fn find_peak(
        &self,
        start_freq: f64,
        end_freq: f64,
        xf: &[f64],
        fft: &[Complex<f64>],
    ) -> (usize, f64) {
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

        return (max_ind, mean / count.max(1.0));
    }

    fn mean_complex(&self, a: &[Complex<f64>]) -> f64 {
        let mut mean_val = 0.0;
        for i in 0..a.len() {
            mean_val += a[i].re;
        }
        return mean_val / (a.len() as f64);
    }

    fn detrend(&self, a: Vec<Complex<f64>>) -> Vec<Complex<f64>> {
        let mean_val = self.mean_complex(&a[..]);
        let mut b: Vec<Complex<f64>> = Vec::new();
        for i in 0..a.len() {
            b.push(Complex::new(a[i].re - mean_val, 0.0));
        }
        return b;
    }

    fn handle_timeout(&mut self) {
        self.handle_drop();
    }
}
