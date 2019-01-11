extern crate serde_json;

use clap::{crate_version, App, Arg};
use log::*;

use solana::client::mk_client;
use solana::cluster_info::{Node, NodeInfo, FULLNODE_PORT_RANGE};
use solana::fullnode::{Fullnode, FullnodeReturnType};
use solana::leader_scheduler::LeaderScheduler;
use solana::local_vote_signer_service::LocalVoteSignerService;
use solana::service::Service;
use solana::socketaddr;
use solana::thin_client::poll_gossip_for_leader;
use solana::vote_signer_proxy::{RemoteVoteSigner, VoteSignerProxy};
use solana_sdk::signature::{Keypair, KeypairUtil};
use solana_sdk::vote_program::VoteProgram;
use solana_sdk::vote_transaction::VoteTransaction;
use std::fs::File;
use std::net::{Ipv4Addr, SocketAddr};
use std::process::exit;
use std::sync::Arc;
use std::thread::sleep;
use std::time::Duration;

fn main() {
    solana_logger::setup();
    solana_metrics::set_panic_hook("fullnode");
    let matches = App::new("fullnode")
        .version(crate_version!())
        .arg(
            Arg::with_name("nosigverify")
                .short("v")
                .long("nosigverify")
                .help("Run without signature verification"),
        )
        .arg(
            Arg::with_name("no-leader-rotation")
                .long("no-leader-rotation")
                .help("Disable leader rotation"),
        )
        .arg(
            Arg::with_name("identity")
                .short("i")
                .long("identity")
                .value_name("PATH")
                .takes_value(true)
                .help("Run with the identity found in FILE"),
        )
        .arg(
            Arg::with_name("network")
                .short("n")
                .long("network")
                .value_name("HOST:PORT")
                .takes_value(true)
                .help("Rendezvous with the network at this gossip entry point"),
        )
        .arg(
            Arg::with_name("signer")
                .short("s")
                .long("signer")
                .value_name("HOST:PORT")
                .takes_value(true)
                .help("Rendezvous with the vote signer at this RPC end point"),
        )
        .arg(
            Arg::with_name("ledger")
                .short("l")
                .long("ledger")
                .value_name("DIR")
                .takes_value(true)
                .required(true)
                .help("Use DIR as persistent ledger location"),
        )
        .arg(
            Arg::with_name("rpc")
                .long("rpc")
                .value_name("PORT")
                .takes_value(true)
                .help("Custom RPC port for this node"),
        )
        .get_matches();

    let nosigverify = matches.is_present("nosigverify");
    let use_only_bootstrap_leader = matches.is_present("no-leader-rotation");

    let (keypair, gossip) = if let Some(i) = matches.value_of("identity") {
        let path = i.to_string();
        if let Ok(file) = File::open(path.clone()) {
            let parse: serde_json::Result<solana_fullnode_config::Config> =
                serde_json::from_reader(file);

            if let Ok(config_data) = parse {
                let keypair = config_data.keypair();
                let node_info = NodeInfo::new_with_pubkey_socketaddr(
                    keypair.pubkey(),
                    &config_data.bind_addr(FULLNODE_PORT_RANGE.0),
                );

                (keypair, node_info.gossip)
            } else {
                eprintln!("failed to parse {}", path);
                exit(1);
            }
        } else {
            eprintln!("failed to read {}", path);
            exit(1);
        }
    } else {
        (Keypair::new(), socketaddr!(0, 8000))
    };

    let ledger_path = matches.value_of("ledger").unwrap();

    // socketaddr that is initial pointer into the network's gossip
    let network = matches
        .value_of("network")
        .map(|network| network.parse().expect("failed to parse network address"));

    let node = Node::new_with_external_ip(keypair.pubkey(), &gossip);

    // save off some stuff for airdrop
    let mut node_info = node.info.clone();

    let keypair = Arc::new(keypair);
    let pubkey = keypair.pubkey();

    let mut leader_scheduler = LeaderScheduler::default();
    leader_scheduler.use_only_bootstrap_leader = use_only_bootstrap_leader;

    let rpc_port = if let Some(port) = matches.value_of("rpc") {
        let port_number = port.to_string().parse().expect("integer");
        if port_number == 0 {
            eprintln!("Invalid RPC port requested: {:?}", port);
            exit(1);
        }
        Some(port_number)
    } else {
        match solana_netutil::find_available_port_in_range(FULLNODE_PORT_RANGE) {
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

    let (signer_service, signer) = if let Some(signer_addr) = matches.value_of("signer") {
        (
            None,
            signer_addr.to_string().parse().expect("Signer IP Address"),
        )
    } else {
        // If a remote vote-signer service is not provided, run a local instance
        let (signer_service, addr) = LocalVoteSignerService::new();
        (Some(signer_service), addr)
    };

    let mut client = mk_client(&leader);
    let vote_signer = VoteSignerProxy::new(&keypair, Box::new(RemoteVoteSigner::new(signer)));
    let vote_account = vote_signer.vote_account;
    info!("New vote account ID is {:?}", vote_account);

    let mut fullnode = Fullnode::new(
        node,
        ledger_path,
        keypair.clone(),
        Arc::new(vote_signer),
        network,
        nosigverify,
        leader_scheduler,
        rpc_port,
    );

    let balance = client.poll_get_balance(&pubkey).unwrap_or(0);
    info!("balance is {}", balance);
    if balance < 1 {
        error!("insufficient tokens, one token required");
        if let Some(signer_service) = signer_service {
            signer_service.join().unwrap();
        }
        exit(1);
    }

    // Create the vote account if necessary
    if client.poll_get_balance(&vote_account).unwrap_or(0) == 0 {
        // Need at least two tokens as one token will be spent on a vote_account_new() transaction
        if balance < 2 {
            error!("insufficient tokens, two tokens required");
            if let Some(signer_service) = signer_service {
                signer_service.join().unwrap();
            }
            exit(1);
        }
        loop {
            let last_id = client.get_last_id();
            let transaction =
                VoteTransaction::vote_account_new(&keypair, vote_account, last_id, 1, 1);
            if client.transfer_signed(&transaction).is_err() {
                sleep(Duration::from_secs(2));
                continue;
            }

            let balance = client.poll_get_balance(&vote_account).unwrap_or(0);
            if balance > 0 {
                break;
            }
            sleep(Duration::from_secs(2));
        }
    }

    loop {
        let vote_account_user_data = client.get_account_userdata(&vote_account);
        if let Ok(Some(vote_account_user_data)) = vote_account_user_data {
            if let Ok(vote_state) = VoteProgram::deserialize(&vote_account_user_data) {
                if vote_state.node_id == pubkey {
                    break;
                }
            }
        }
        panic!("Expected successful vote account registration");
    }

    loop {
        let status = fullnode.handle_role_transition();
        match status {
            Ok(Some(FullnodeReturnType::LeaderToValidatorRotation)) => (),
            Ok(Some(FullnodeReturnType::ValidatorToLeaderRotation)) => (),
            _ => {
                // Fullnode tpu/tvu exited for some unexpected
                // reason, so exit
                if let Some(signer_service) = signer_service {
                    signer_service.join().unwrap();
                }
                exit(1);
            }
        }
    }
}
