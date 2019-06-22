extern crate time;
#[macro_use]
extern crate slog;

extern crate ccp_nimbus;
extern crate portus;
extern crate structopt;

use ccp_nimbus::{Nimbus, NimbusConfig};
use portus::ipc::{BackendBuilder, Blocking};

use structopt::StructOpt;

fn main() {
    let log = portus::algs::make_logger();
    let cfg = NimbusConfig::from_args();

    info!(log, "starting CCP";
        "algorithm" => "NIMBUS",
        "ipc" => cfg.ipc.clone(),
    );

    match cfg.ipc.as_str() {
        "unix" => {
            use portus::ipc::unix::Socket;
            let b = Socket::<Blocking>::new("in", "out")
                .map(|sk| BackendBuilder { sock: sk })
                .expect("ipc initialization");
            portus::run::<_, Nimbus<_>>(
                b,
                &portus::Config {
                    logger: Some(log),
                    config: cfg,
                },
            )
            .unwrap();
        }
        #[cfg(all(target_os = "linux"))]
        "netlink" => {
            use portus::ipc::netlink::Socket;
            let b = Socket::<Blocking>::new()
                .map(|sk| BackendBuilder { sock: sk })
                .expect("ipc initialization");
            portus::run::<_, Nimbus<_>>(
                b,
                &portus::Config {
                    logger: Some(log),
                    config: cfg,
                },
            )
            .unwrap();
        }
        #[cfg(all(target_os = "linux"))]
        "char" => {
            use portus::ipc::kp::Socket;
            let b = Socket::<Blocking>::new()
                .map(|sk| BackendBuilder { sock: sk })
                .expect("char initialization");
            portus::run::<_, Nimbus<_>>(
                b,
                &portus::Config {
                    logger: Some(log),
                    config: cfg,
                },
            )
            .unwrap()
        }
        _ => unreachable!(),
    }
}
