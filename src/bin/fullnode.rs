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
    let repl_clone = repl_data.clone();

    let ledger_path = matches.value_of("ledger").unwrap();

    let mut node = TestNode::new_with_bind_addr(repl_data, bind_addr);
    let mut drone_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), DRONE_PORT);
    let fullnode = if let Some(t) = matches.value_of("testnet") {
        let testnet_address_string = t.to_string();
        let testnet_addr: SocketAddr = testnet_address_string.parse().unwrap();
        drone_addr.set_ip(testnet_addr.ip());

        Fullnode::new(node, ledger_path, keypair, Some(testnet_addr), false)
    } else {
        node.data.leader_id = node.data.id;

        Fullnode::new(node, ledger_path, keypair, None, false)
    };

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

        // Try multiple times to confirm a non-zero balance.  |poll_get_balance| currently times
        // out after 1 second, and sometimes this is not enough time while the network is
        // booting
        let balance_ok = (0..30).any(|i| {
            let balance = client.poll_get_balance(&leader_pubkey).unwrap();
            eprintln!("new balance is {} (attempt #{})", balance, i);
            balance > 0
        });
        assert!(balance_ok, "0 balance, airdrop failed?");
    }

    fullnode.join().expect("join");
}
