#[macro_use]
extern crate clap;
use clap::Arg;
extern crate time;
#[macro_use]
extern crate slog;

extern crate ccp_nimbus;
extern crate portus;

use ccp_nimbus::{Nimbus, FlowMode, LossMode, DelayMode};
use portus::ipc::{BackendBuilder, Blocking};

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
             .possible_values(&FlowMode::variants())
             .case_insensitive(true)
             .default_value("XTCP")
             .help(""))
        .arg(Arg::with_name("delay_mode")
             .long("delay_mode")
             .possible_values(&DelayMode::variants())
             .case_insensitive(true)
             .default_value("Nimbus")
             .help(""))
        .arg(Arg::with_name("loss_mode")
             .long("loss_mode")
             .possible_values(&LossMode::variants())
             .case_insensitive(true)
             .default_value("MulTCP")
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
            use_switching_arg: value_t!(matches, "use_switching", bool).unwrap(),
            bw_est_mode_arg: value_t!(matches, "bw_est_mode", bool).unwrap(),
            delay_threshold_arg: value_t!(matches, "delay_threshold", f64).unwrap(),
            xtcp_flows_arg: value_t!(matches, "xtcp_flows", i32).unwrap(),
            init_delay_threshold_arg: value_t!(matches, "init_delay_threshold", f64).unwrap(),
            frequency_arg: value_t!(matches, "frequency", f64).unwrap(),
            pulse_size_arg: value_t!(matches, "pulse_size", f64).unwrap(),
            switching_thresh_arg: value_t!(matches, "switching_thresh", f64).unwrap(),
            flow_mode_arg: value_t!(matches, "flow_mode", FlowMode).unwrap(),
            delay_mode_arg: value_t!(matches, "delay_mode", DelayMode).unwrap(),
            loss_mode_arg: value_t!(matches, "loss_mode", LossMode).unwrap(),
            uest_arg: value_t!(matches, "uest", f64).unwrap() * 125_000f64,
            use_ewma_arg: value_t!(matches, "use_ewma", bool).unwrap(),
            set_win_cap_arg: value_t!(matches, "set_win_cap", bool).unwrap(),
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
