#[macro_use]
extern crate slog;

use ccp_nimbus::{Nimbus, NimbusConfig};
use portus::ipc::{BackendBuilder, Blocking};
use structopt::StructOpt;

use portus::ipc::unix::Socket as S;
use portus::ipc::Blocking as B;

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

    let portus_bindaddr = format!("{}/core{}/{}", "1", "0", "portus");
    let backend = S::<B>::new(&portus_bindaddr, 10485760, 10485760)
        .map(|sk| portus::ipc::BackendBuilder { sock : sk })
        .expect("ipc initialization");
    let rb = portus::RunBuilder::new(backend, portus::Config { logger : Some (log.clone()) })
       .default_alg(cfg);

    let _ = rb.run();
}
