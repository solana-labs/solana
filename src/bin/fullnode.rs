extern crate atty;
extern crate clap;
extern crate env_logger;
extern crate getopts;
extern crate log;
extern crate serde_json;
extern crate solana;

use atty::{is, Stream};
use clap::{App, Arg};
use solana::crdt::{NodeInfo, TestNode};
use solana::fullnode::{Config, FullNode, Ledger};
use solana::service::Service;
use solana::signature::{KeyPair, KeyPairUtil};
use std::fs::File;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::process::exit;
//use std::time::Duration;

fn main() -> () {
    env_logger::init();
    let matches = App::new("fullnode")
        .arg(
            Arg::with_name("identity")
                .short("l")
                .long("local")
                .value_name("FILE")
                .takes_value(true)
                .help("run with the identity found in FILE"),
        )
        .arg(
            Arg::with_name("testnet")
                .short("t")
                .long("testnet")
                .value_name("HOST:PORT")
                .takes_value(true)
                .help("testnet; connect to the network at this gossip entry point"),
        )
        .arg(
            Arg::with_name("ledger")
                .short("L")
                .long("ledger")
                .value_name("FILE")
                .takes_value(true)
                .help("use FILE as persistent ledger (defaults to stdin/stdout)"),
        )
        .get_matches();
    if is(Stream::Stdin) {
        eprintln!("nothing found on stdin, expected a log file");
        exit(1);
    }

    let bind_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), 8000);
    let mut keypair = KeyPair::new();
    let mut repl_data = NodeInfo::new_leader_with_pubkey(keypair.pubkey(), &bind_addr);
    if let Some(l) = matches.value_of("identity") {
        let path = l.to_string();
        if let Ok(file) = File::open(path.clone()) {
            let parse: serde_json::Result<Config> = serde_json::from_reader(file);
            if let Ok(data) = parse {
                keypair = data.keypair();
                repl_data = data.node_info;
            } else {
                eprintln!("failed to parse {}", path);
                exit(1);
            }
        } else {
            eprintln!("failed to read {}", path);
            exit(1);
        }
    }
    let ledger = if let Some(l) = matches.value_of("ledger") {
        Ledger::Path(l.to_string())
    } else {
        Ledger::StdInOut
    };

    let mut node = TestNode::new_with_bind_addr(repl_data, bind_addr);
    let fullnode = if let Some(t) = matches.value_of("testnet") {
        let testnet_address_string = t.to_string();
        let testnet_addr = testnet_address_string.parse().unwrap();

        FullNode::new(node, false, ledger, Some(keypair), Some(testnet_addr))
    } else {
        node.data.leader_id = node.data.id;

        FullNode::new(node, true, ledger, None, None)
    };
    fullnode.join().expect("join");
}
