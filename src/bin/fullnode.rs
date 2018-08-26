#[macro_use]
extern crate clap;
extern crate getopts;
extern crate log;
extern crate serde_json;
extern crate solana;

use clap::{App, Arg};
use solana::client::mk_client;
use solana::crdt::{NodeInfo, TestNode};
use solana::drone::DRONE_PORT;
use solana::fullnode::{Config, Fullnode};
use solana::logger;
use solana::metrics::set_panic_hook;
use solana::service::Service;
use solana::signature::{Keypair, KeypairUtil};
use solana::wallet::request_airdrop;
use std::fs::File;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::process::exit;
use std::time::Duration;

fn main() -> () {
    logger::setup();
    set_panic_hook("fullnode");
    let matches = App::new("fullnode")
        .version(crate_version!())
        .arg(
            Arg::with_name("identity")
                .short("i")
                .long("identity")
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
                .help("connect to the network at this gossip entry point"),
        )
        .arg(
            Arg::with_name("ledger")
                .short("l")
                .long("ledger")
                .value_name("DIR")
                .takes_value(true)
                .required(true)
                .help("use DIR as persistent ledger location"),
        )
        .get_matches();

    let bind_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), 8000);
    let mut keypair = Keypair::new();
    let mut repl_data = NodeInfo::new_leader_with_pubkey(keypair.pubkey(), &bind_addr);
    if let Some(i) = matches.value_of("identity") {
        let path = i.to_string();
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

    let leader_pubkey = keypair.pubkey();

    let ledger_path = matches.value_of("ledger").unwrap();

    let port_range = (8100, 10000);
    let node = if let Some(_t) = matches.value_of("testnet") {
        TestNode::new_with_external_ip(
            leader_pubkey,
            repl_data.contact_info.ncp.ip(),
            port_range,
            0,
        )
    } else {
        TestNode::new_with_external_ip(
            leader_pubkey,
            repl_data.contact_info.ncp.ip(),
            port_range,
            repl_data.contact_info.ncp.port(),
        )
    };
    let repl_clone = node.data.clone();

    let mut drone_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), DRONE_PORT);
    let testnet_addr = matches.value_of("testnet").map(|addr_str| {
        let addr: SocketAddr = addr_str.parse().unwrap();
        drone_addr.set_ip(addr.ip());
        addr
    });
    let fullnode = Fullnode::new(node, ledger_path, keypair, testnet_addr, false);

    let mut client = mk_client(&repl_clone);
    let previous_balance = client.poll_get_balance(&leader_pubkey).unwrap_or(0);
    eprintln!("balance is {}", previous_balance);

    if previous_balance == 0 {
        eprintln!("requesting airdrop from {}", drone_addr);
        request_airdrop(&drone_addr, &leader_pubkey, 50).unwrap_or_else(|_| {
            panic!(
                "Airdrop failed, is the drone address correct {:?} drone running?",
                drone_addr
            )
        });

        let balance_ok = client
            .poll_balance_with_timeout(
                &leader_pubkey,
                &Duration::from_millis(100),
                &Duration::from_secs(30),
            )
            .unwrap() > 0;
        assert!(balance_ok, "0 balance, airdrop failed?");
    }

    fullnode.join().expect("join");
}
