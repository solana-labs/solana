#[macro_use]
extern crate clap;
extern crate getopts;
extern crate serde_json;
#[macro_use]
extern crate solana;

use clap::{App, Arg};
use solana::crdt::Node;
use solana::fullnode::Config;
use solana::logger;
use solana::replicator::Replicator;
use solana::signature::{Keypair, KeypairUtil};
use std::fs::File;
use std::net::{Ipv4Addr, SocketAddr};
use std::process::exit;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

fn main() {
    logger::setup();
    let matches = App::new("replicator")
        .version(crate_version!())
        .arg(
            Arg::with_name("identity")
                .short("i")
                .long("identity")
                .value_name("PATH")
                .takes_value(true)
                .help("Run with the identity found in FILE"),
        ).arg(
            Arg::with_name("network")
                .short("n")
                .long("network")
                .value_name("HOST:PORT")
                .takes_value(true)
                .help("Rendezvous with the network at this gossip entry point"),
        ).arg(
            Arg::with_name("ledger")
                .short("l")
                .long("ledger")
                .value_name("DIR")
                .takes_value(true)
                .required(true)
                .help("use DIR as persistent ledger location"),
        ).get_matches();

    let ledger_path = matches.value_of("ledger");

    let (keypair, ncp) = if let Some(i) = matches.value_of("identity") {
        let path = i.to_string();
        if let Ok(file) = File::open(path.clone()) {
            let parse: serde_json::Result<Config> = serde_json::from_reader(file);
            if let Ok(data) = parse {
                (data.keypair(), data.node_info.contact_info.ncp)
            } else {
                eprintln!("failed to parse {}", path);
                exit(1);
            }
        } else {
            eprintln!("failed to read {}", path);
            exit(1);
        }
    } else {
        (Keypair::new(), socketaddr!([127, 0, 0, 1], 8700))
    };

    let node = Node::new_with_external_ip(keypair.pubkey(), &ncp);

    println!(
        "replicating the data with keypair: {:?} ncp:{:?}",
        keypair.pubkey(),
        ncp
    );
    println!("my node: {:?}", node);

    let exit = Arc::new(AtomicBool::new(false));

    let network_addr = matches
        .value_of("network")
        .map(|network| network.parse().expect("failed to parse network address"));

    // TODO: ask network what slice we should store
    let entry_height = 0;

    let replicator = Replicator::new(entry_height, &exit, ledger_path, node, network_addr);

    replicator.join();
}
