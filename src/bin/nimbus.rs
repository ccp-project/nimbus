use ccp_nimbus::{Nimbus, NimbusConfig};
use portus::ipc::{BackendBuilder, Blocking};
use structopt::StructOpt;
use tracing::info;

fn main() {
    let cfg = NimbusConfig::from_args();
    let ipc = cfg.ipc.clone();
    let nimbus: Nimbus = cfg.into();

    info!(
        algorithm = "NIMBUS",
        ipc = ?&ipc,
        "starting CCP"
    );

    match ipc.as_str() {
        "unix" => {
            use portus::ipc::unix::Socket;
            let b = Socket::<Blocking>::new("in", "out")
                .map(|sk| BackendBuilder { sock: sk })
                .expect("ipc initialization");
            portus::run(b, portus::Config { logger: None }, nimbus).unwrap();
        }
        #[cfg(all(target_os = "linux"))]
        "netlink" => {
            use portus::ipc::netlink::Socket;
            let b = Socket::<Blocking>::new()
                .map(|sk| BackendBuilder { sock: sk })
                .expect("ipc initialization");
            portus::run(b, portus::Config { logger: None }, nimbus).unwrap();
        }
        #[cfg(all(target_os = "linux"))]
        "char" => {
            use portus::ipc::kp::Socket;
            let b = Socket::<Blocking>::new()
                .map(|sk| BackendBuilder { sock: sk })
                .expect("char initialization");
            portus::run(b, portus::Config { logger: None }, nimbus).unwrap()
        }
        _ => unreachable!(),
    }
}
