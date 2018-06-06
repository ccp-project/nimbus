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
        .version("0.1.0")
        .author("Prateesh Goyal <prateesh@mit.edu>")
        .about("Implementation of NIMBUS Congestion Control")
        .arg(Arg::with_name("ipc")
             .long("ipc")
             .help("Sets the type of ipc to use: (netlink|unix)")
             .default_value("unix")
             .validator(portus::algs::ipc_valid))
        .arg(Arg::with_name("useSwitching")
             .long("useSwitching")
             .default_value("false")
             .help(""))
        .arg(Arg::with_name("bwEstMode")
             .long("bwEstMode")
             .default_value("true")
             .help(""))
        .arg(Arg::with_name("delayThreshold")
             .long("delayThreshold")
             .default_value("1.25")
             .help(""))
        .arg(Arg::with_name("xtcpFlows")
             .long("xtcpFlows")
             .default_value("1")
             .help(""))
        .arg(Arg::with_name("initDelayThreshold")
             .long("initDelayThreshold")
             .default_value("1.25")
             .help(""))
        .arg(Arg::with_name("frequency")
             .long("frequency")
             .default_value("5.0")
             .help(""))
        .arg(Arg::with_name("pulseSize")
             .long("pulseSize")
             .default_value("0.25")
             .help(""))
        .arg(Arg::with_name("switchingThresh")
             .long("switchingThresh")
             .default_value("0.4")
             .help(""))
        .arg(Arg::with_name("flowMode")
             .long("flowMode")
             .default_value("XTCP")
             .help(""))
        .arg(Arg::with_name("delayMode")
             .long("delayMode")
             .default_value("Nimbus")
             .help(""))
        .arg(Arg::with_name("lossMode")
             .long("lossMode")
             .default_value("Cubic")
            .help(""))
        .arg(Arg::with_name("uest")
             .long("uest")
             .default_value("96.0")
             .help(""))
        .arg(Arg::with_name("useEWMA")
             .long("useEWMA")
             .default_value("false")
             .help(""))
        .arg(Arg::with_name("setWinCap")
             .long("setWinCap")
             .default_value("false")
             .help(""))
        .get_matches();



    Ok((
        ccp_nimbus::NimbusConfig {
            useSwitchingArg: pry_arg!(matches, "useSwitching", bool),
            bwEstModeArg: pry_arg!(matches, "bwEstMode", bool),
            delayThresholdArg: pry_arg!(matches, "delayThreshold", f64),
            xtcpFlowsArg: pry_arg!(matches, "xtcpFlows", i32),
            initDelayThresholdArg: pry_arg!(matches, "initDelayThreshold", f64),
            frequencyArg: pry_arg!(matches, "frequency", f64),
            pulseSizeArg: pry_arg!(matches, "pulseSize", f64),
            switchingThreshArg: pry_arg!(matches, "switchingThresh", f64),
            flowModeArg: matches.value_of("flowMode").unwrap().to_string(),
            delayModeArg: matches.value_of("delayMode").unwrap().to_string(),
            lossModeArg: matches.value_of("lossMode").unwrap().to_string(),
            uestArg: pry_arg!(matches, "uest", f64) * 125000f64,
            useEWMAArg: pry_arg!(matches, "useEWMA"),
            setWinCapArg: pry_arg!(matches, "setWinCap"),
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
