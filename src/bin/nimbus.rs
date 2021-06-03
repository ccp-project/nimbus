#[macro_use]
extern crate slog;

use ccp_nimbus::{Nimbus, NimbusConfig};
use portus::ipc::{BackendBuilder, Blocking};
use structopt::StructOpt;

fn main() {
    let log = portus::algs::make_logger();
    let cfg = NimbusConfig::from_args();
    let ipc = cfg.ipc.clone();
    let mut nimbus: Nimbus = cfg.into();
    nimbus.with_logger(log.clone());

    info!(&log, "starting CCP";
        "algorithm" => "NIMBUS",
        "ipc" => ?&ipc,
    );

    match ipc.as_str() {
        "unix" => {
            use portus::ipc::unix::Socket;
            let b = Socket::<Blocking>::new("in", "out")
                .map(|sk| BackendBuilder { sock: sk })
                .expect("ipc initialization");
            portus::run(b, portus::Config { logger: Some(log) }, nimbus).unwrap();
        }
        #[cfg(all(target_os = "linux"))]
        "netlink" => {
            use portus::ipc::netlink::Socket;
            let b = Socket::<Blocking>::new()
                .map(|sk| BackendBuilder { sock: sk })
                .expect("ipc initialization");
            portus::run(b, portus::Config { logger: Some(log) }, nimbus).unwrap();
        }
        #[cfg(all(target_os = "linux"))]
        "char" => {
            use portus::ipc::kp::Socket;
            let b = Socket::<Blocking>::new()
                .map(|sk| BackendBuilder { sock: sk })
                .expect("char initialization");
            portus::run(b, portus::Config { logger: Some(log) }, nimbus).unwrap()
        }
        _ => unreachable!(),
    }
}
