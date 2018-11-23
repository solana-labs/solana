#[macro_use]
extern crate clap;
extern crate getopts;
#[macro_use]
extern crate log;
extern crate serde_json;
#[macro_use]
extern crate solana;
extern crate rocksdb;
extern crate solana_metrics;

use clap::{App, Arg};
use rocksdb::{Options, DB};
use solana::client::mk_client;
use solana::cluster_info::{Node, FULLNODE_PORT_RANGE};
use solana::db_ledger::{write_entries_to_ledger, DB_LEDGER_DIRECTORY};
use solana::fullnode::{Config, Fullnode, FullnodeReturnType};
use solana::leader_scheduler::LeaderScheduler;
use solana::ledger::read_ledger;
use solana::logger;
use solana::netutil::find_available_port_in_range;
use solana::signature::{Keypair, KeypairUtil};
use solana::thin_client::poll_gossip_for_leader;
use solana::vote_program::VoteProgram;
use solana::vote_transaction::VoteTransaction;
use std::fs::File;
use std::net::{Ipv4Addr, SocketAddr};
use std::process::exit;
use std::sync::Arc;
use std::thread::sleep;
use std::time::Duration;

fn main() {
    logger::setup();
    solana_metrics::set_panic_hook("fullnode");
    let matches = App::new("fullnode")
        .version(crate_version!())
        .arg(
            Arg::with_name("nosigverify")
                .short("v")
                .long("nosigverify")
                .help("Run without signature verification"),
        ).arg(
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
                .help("Use DIR as persistent ledger location"),
        ).arg(
            Arg::with_name("rpc")
                .long("rpc")
                .value_name("PORT")
                .takes_value(true)
                .help("Custom RPC port for this node"),
        ).get_matches();

    let sigverify = !matches.is_present("nosigverify");

    let (keypair, vote_account_keypair, ncp) = if let Some(i) = matches.value_of("identity") {
        let path = i.to_string();
        if let Ok(file) = File::open(path.clone()) {
            let parse: serde_json::Result<Config> = serde_json::from_reader(file);
            if let Ok(data) = parse {
                (
                    data.keypair(),
                    data.vote_account_keypair(),
                    data.node_info.ncp,
                )
            } else {
                eprintln!("failed to parse {}", path);
                exit(1);
            }
        } else {
            eprintln!("failed to read {}", path);
            exit(1);
        }
    } else {
        (Keypair::new(), Keypair::new(), socketaddr!(0, 8000))
    };

    let ledger_path = matches.value_of("ledger").unwrap();

    // Create the RocksDb ledger, eventually will simply repurpose the input
    // ledger path as the RocksDb ledger path
    let db_ledger_path = format!("{}/{}", ledger_path, DB_LEDGER_DIRECTORY);
    // Destroy old ledger
    DB::destroy(&Options::default(), &db_ledger_path)
        .expect("Expected successful database destruction");
    let ledger_entries: Vec<_> = read_ledger(ledger_path, true)
        .expect("opening ledger")
        .map(|entry| entry.unwrap())
        .collect();
    write_entries_to_ledger(&[db_ledger_path], &ledger_entries[..]);

    // socketaddr that is initial pointer into the network's gossip (ncp)
    let network = matches
        .value_of("network")
        .map(|network| network.parse().expect("failed to parse network address"));

    let node = Node::new_with_external_ip(keypair.pubkey(), &ncp);

    // save off some stuff for airdrop
    let mut node_info = node.info.clone();

    let vote_account_keypair = Arc::new(vote_account_keypair);
    let vote_account_id = vote_account_keypair.pubkey();
    let keypair = Arc::new(keypair);
    let pubkey = keypair.pubkey();

    let mut leader_scheduler = LeaderScheduler::default();

    // Remove this line to enable leader rotation
    leader_scheduler.use_only_bootstrap_leader = true;

    let rpc_port = if let Some(port) = matches.value_of("rpc") {
        let port_number = port.to_string().parse().expect("integer");
        if port_number == 0 {
            eprintln!("Invalid RPC port requested: {:?}", port);
            exit(1);
        }
        Some(port_number)
    } else {
        match find_available_port_in_range(FULLNODE_PORT_RANGE) {
            Ok(port) => Some(port),
            Err(_) => None,
        }
    };

    let leader = match network {
        Some(network) => {
            poll_gossip_for_leader(network, None).expect("can't find leader on network")
        }
        None => {
            //self = leader
            if rpc_port.is_some() {
                node_info.rpc.set_port(rpc_port.unwrap());
                node_info.rpc_pubsub.set_port(rpc_port.unwrap() + 1);
            }
            node_info
        }
    };

    let mut fullnode = Fullnode::new(
        node,
        ledger_path,
        keypair.clone(),
        vote_account_keypair.clone(),
        network,
        sigverify,
        leader_scheduler,
        rpc_port,
    );
    let mut client = mk_client(&leader);

    let balance = client.poll_get_balance(&pubkey).unwrap_or(0);
    info!("balance is {}", balance);
    if balance < 1 {
        error!("insufficient tokens");
        exit(1);
    }

    // Create the vote account if necessary
    if client.poll_get_balance(&vote_account_id).unwrap_or(0) == 0 {
        // Need at least two tokens as one token will be spent on a vote_account_new() transaction
        if balance < 2 {
            error!("insufficient tokens");
            exit(1);
        }
        loop {
            let last_id = client.get_last_id();
            let transaction =
                VoteTransaction::vote_account_new(&keypair, vote_account_id, last_id, 1);
            if client.transfer_signed(&transaction).is_err() {
                sleep(Duration::from_secs(2));
                continue;
            }

            let balance = client.poll_get_balance(&vote_account_id).unwrap_or(0);
            if balance > 0 {
                break;
            }
            sleep(Duration::from_secs(2));
        }
    }

    // Register the vote account to this node
    loop {
        let last_id = client.get_last_id();
        let transaction =
            VoteTransaction::vote_account_register(&keypair, vote_account_id, last_id, 0);
        if client.transfer_signed(&transaction).is_err() {
            sleep(Duration::from_secs(2));
            continue;
        }

        let account_user_data = client.get_account_userdata(&vote_account_id);
        if let Ok(Some(account_user_data)) = account_user_data {
            if let Ok(vote_state) = VoteProgram::deserialize(&account_user_data) {
                if vote_state.node_id == pubkey {
                    break;
                }
            }
        }

        sleep(Duration::from_secs(2));
    }

    loop {
        let status = fullnode.handle_role_transition();
        match status {
            Ok(Some(FullnodeReturnType::LeaderToValidatorRotation)) => (),
            Ok(Some(FullnodeReturnType::ValidatorToLeaderRotation)) => (),
            _ => {
                // Fullnode tpu/tvu exited for some unexpected
                // reason, so exit
                exit(1);
            }
        }
    }
}
