use clap::{crate_description, crate_name, App, Arg};
use solana_vote_signer::rpc::VoteSignerRpcService;
use std::error;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
pub const RPC_PORT: u16 = 8989;

fn main() -> Result<(), Box<dyn error::Error>> {
    solana_metrics::set_panic_hook("vote-signer");

    let matches = App::new(crate_name!())
        .about(crate_description!())
        .version(solana_clap_utils::version!())
        .arg(
            Arg::with_name("port")
                .long("port")
                .value_name("NUM")
                .takes_value(true)
                .help("JSON RPC listener port"),
        )
        .get_matches();

    let port = if let Some(p) = matches.value_of("port") {
        p.to_string()
            .parse()
            .expect("Failed to parse JSON RPC Port")
    } else {
        RPC_PORT
    };

    let exit = Arc::new(AtomicBool::new(false));
    let service = VoteSignerRpcService::new(
        SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), port),
        &exit,
    );

    service.join().unwrap();
    Ok(())
}
