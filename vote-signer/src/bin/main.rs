#[macro_use]
extern crate clap;
extern crate log;
extern crate solana_metrics;
extern crate solana_sdk;
extern crate solana_vote_signer;

use clap::{App, Arg};
use solana_vote_signer::rpc::VoteSignerRpcService;
use std::error;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};

pub const RPC_PORT: u16 = 8989;

fn main() -> Result<(), Box<error::Error>> {
    solana_metrics::set_panic_hook("vote-signer");

    let matches = App::new("vote-signer")
        .version(crate_version!())
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

    let service =
        VoteSignerRpcService::new(SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), port));

    service.join().unwrap();

    Ok(())
}
