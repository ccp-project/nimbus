extern crate clap;
use clap::Arg;
extern crate time;
#[macro_use]
extern crate slog;

extern crate ccp_nimbus;
extern crate portus;

use ccp_nimbus::Nimbus;
use portus::ipc::{BackendBuilder, Blocking};

macro_rules! pry_arg {
    ($m:expr, $s:expr) => ($m.value_of($s).unwrap().parse().unwrap());
    ($m:expr, $s:expr, $t:ty) => ($m.value_of($s).unwrap().parse::<$t>().unwrap());
}

fn make_args() -> Result<(ccp_nimbus::NimbusConfig, String), String> {
    let matches = clap::App::new("CCP NIMBUS")
        .version("0.2.1")
        .author("Prateesh Goyal <prateesh@mit.edu>")
        .about("Implementation of NIMBUS Congestion Control")
        .arg(Arg::with_name("ipc")
             .long("ipc")
             .help("Sets the type of ipc to use: (netlink|unix)")
             .default_value("unix")
             .validator(portus::algs::ipc_valid))
        .arg(Arg::with_name("use_switching")
             .long("use_switching")
             .default_value("false")
             .help(""))
        .arg(Arg::with_name("bw_est_mode")
             .long("bw_est_mode")
             .default_value("true")
             .help(""))
        .arg(Arg::with_name("delay_threshold")
             .long("delay_threshold")
             .default_value("1.25")
             .help(""))
        .arg(Arg::with_name("xtcp_flows")
             .long("xtcp_flows")
             .default_value("1")
             .help(""))
        .arg(Arg::with_name("init_delay_threshold")
             .long("init_delay_threshold")
             .default_value("1.25")
             .help(""))
        .arg(Arg::with_name("frequency")
             .long("frequency")
             .default_value("5.0")
             .help(""))
        .arg(Arg::with_name("pulse_size")
             .long("pulse_size")
             .default_value("0.25")
             .help(""))
        .arg(Arg::with_name("switching_thresh")
             .long("switching_thresh")
             .default_value("0.4")
             .help(""))
        .arg(Arg::with_name("flow_mode")
             .long("flow_mode")
             .default_value("XTCP")
             .help(""))
        .arg(Arg::with_name("delay_mode")
             .long("delay_mode")
             .default_value("Nimbus")
             .help(""))
        .arg(Arg::with_name("loss_mode")
             .long("loss_mode")
             .default_value("Cubic")
            .help(""))
        .arg(Arg::with_name("uest")
             .long("uest")
             .default_value("96.0")
             .help(""))
        .arg(Arg::with_name("use_ewma")
             .long("use_ewma")
             .default_value("false")
             .help(""))
        .arg(Arg::with_name("set_win_cap")
             .long("set_win_cap")
             .default_value("false")
             .help(""))
        .get_matches();

    Ok((
        ccp_nimbus::NimbusConfig {
            use_switching_arg: pry_arg!(matches, "use_switching", bool),
            bw_est_mode_arg: pry_arg!(matches, "bw_est_mode", bool),
            delay_threshold_arg: pry_arg!(matches, "delay_threshold", f64),
            xtcp_flows_arg: pry_arg!(matches, "xtcp_flows", i32),
            init_delay_threshold_arg: pry_arg!(matches, "init_delay_threshold", f64),
            frequency_arg: pry_arg!(matches, "frequency", f64),
            pulse_size_arg: pry_arg!(matches, "pulse_size", f64),
            switching_thresh_arg: pry_arg!(matches, "switching_thresh", f64),
            flow_mode_arg: matches.value_of("flow_mode").unwrap().to_string(),
            delay_mode_arg: matches.value_of("delay_mode").unwrap().to_string(),
            loss_mode_arg: matches.value_of("loss_mode").unwrap().to_string(),
            uest_arg: pry_arg!(matches, "uest", f64) * 125000f64,
            use_ewma_arg: pry_arg!(matches, "use_ewma"),
            set_win_cap_arg: pry_arg!(matches, "set_win_cap"),
        },
        String::from(matches.value_of("ipc").unwrap()),
    ))
}

fn main() {
    let log = portus::algs::make_logger();
    let (cfg, ipc) = make_args()
        .map_err(|e| warn!(log, "bad argument"; "err" => ?e))
        .unwrap_or_default();

    info!(log, "starting CCP"; 
        "algorithm" => "NIMBUS",
        "ipc" => ipc.clone(),
    );
	match ipc.as_str() {
        "unix" => {
            use portus::ipc::unix::Socket;
            let b = Socket::<Blocking>::new("in", "out")
                .map(|sk| BackendBuilder {sock: sk})
                .expect("ipc initialization");
            portus::run::<_, Nimbus<_>>(
                b,
                &portus::Config {
                    logger: Some(log),
                    config: cfg,
                }
            ).unwrap();
        }
        #[cfg(all(target_os = "linux"))]
        "netlink" => {
            use portus::ipc::netlink::Socket;
            let b = Socket::<Blocking>::new()
                .map(|sk| BackendBuilder {sock: sk})
                .expect("ipc initialization");
            portus::run::<_, Nimbus<_>>(
                b,
                &portus::Config {
                    logger: Some(log),
                    config: cfg,
                }
            ).unwrap();
        }
        #[cfg(all(target_os = "linux"))]
        "char" => {
            use portus::ipc::kp::Socket;
            let b = Socket::<Blocking>::new()
                .map(|sk| BackendBuilder {sock: sk})
                .expect("char initialization");
            portus::run::<_, Nimbus<_>>(
                b,
                &portus::Config {
                    logger: Some(log),
                    config: cfg,
                }
            ).unwrap()
        }
        _ => unreachable!(),
    }
            
}
