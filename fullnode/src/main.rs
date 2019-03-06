use clap::{crate_version, App, Arg};
use log::*;
use solana::client::mk_client;
use solana::cluster_info::{Node, NodeInfo, FULLNODE_PORT_RANGE};
use solana::fullnode::{Fullnode, FullnodeConfig};
use solana::local_vote_signer_service::LocalVoteSignerService;
use solana::service::Service;
use solana::thin_client::{poll_gossip_for_leader, ThinClient};
use solana::voting_keypair::{RemoteVoteSigner, VotingKeypair};
use solana_sdk::genesis_block::GenesisBlock;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{read_keypair, Keypair, KeypairUtil};
use solana_vote_api::vote_state::VoteState;
use solana_vote_api::vote_transaction::VoteTransaction;
use solana_vote_signer::rpc::{LocalVoteSigner, VoteSigner};
use std::fs::File;
use std::io::{Error, ErrorKind, Result};
use std::process::exit;
use std::sync::Arc;

fn create_and_fund_vote_account(
    client: &mut ThinClient,
    vote_account: Pubkey,
    node_keypair: &Arc<Keypair>,
) -> Result<()> {
    let pubkey = node_keypair.pubkey();
    let node_balance = client.poll_get_balance(&pubkey)?;
    info!("node balance is {}", node_balance);
    if node_balance < 1 {
        return Err(Error::new(
            ErrorKind::Other,
            "insufficient lamports, one lamport required",
        ));
    }

    // Create the vote account if necessary
    if client.poll_get_balance(&vote_account).unwrap_or(0) == 0 {
        // Need at least two lamports as one lamport will be spent on a vote_account_new() transaction
        if node_balance < 2 {
            error!("insufficient lamports, two lamports required");
            return Err(Error::new(
                ErrorKind::Other,
                "insufficient lamports, two lamports required",
            ));
        }
        loop {
            let blockhash = client.get_recent_blockhash();
            info!("create_and_fund_vote_account blockhash={:?}", blockhash);
            let transaction =
                VoteTransaction::new_account(node_keypair, vote_account, blockhash, 1, 1);

            match client.transfer_signed(&transaction) {
                Ok(signature) => {
                    match client.poll_for_signature(&signature) {
                        Ok(_) => match client.poll_get_balance(&vote_account) {
                            Ok(balance) => {
                                info!("vote account balance: {}", balance);
                                break;
                            }
                            Err(e) => {
                                info!("Failed to get vote account balance: {:?}", e);
                            }
                        },
                        Err(e) => {
                            info!(
                                "vote_account_new signature not found: {:?}: {:?}",
                                signature, e
                            );
                        }
                    };
                }
                Err(e) => {
                    info!("Failed to send vote_account_new transaction: {:?}", e);
                }
            };
        }
    }

    info!("Checking for vote account registration");
    let vote_account_user_data = client.get_account_userdata(&vote_account);
    if let Ok(Some(vote_account_user_data)) = vote_account_user_data {
        if let Ok(vote_state) = VoteState::deserialize(&vote_account_user_data) {
            if vote_state.delegate_id == vote_account {
                return Ok(());
            }
        }
    }

    Err(Error::new(
        ErrorKind::Other,
        "expected successful vote account registration",
    ))
}

fn main() {
    solana_logger::setup();
    solana_metrics::set_panic_hook("fullnode");

    let matches = App::new("solana-fullnode")
        .version(crate_version!())
        .arg(
            Arg::with_name("blockstream")
                .long("blockstream")
                .takes_value(true)
                .value_name("UNIX DOMAIN SOCKET")
                .help("Open blockstream at this unix domain socket location")
        )
        .arg(
            Arg::with_name("identity")
                .short("i")
                .long("identity")
                .value_name("PATH")
                .takes_value(true)
                .help("File containing an identity (keypair)"),
        )
        .arg(
            Arg::with_name("init_complete_file")
                .long("init-complete-file")
                .value_name("FILE")
                .takes_value(true)
                .help("Create this file, if it doesn't already exist, once node initialization is complete"),
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
            Arg::with_name("network")
                .short("n")
                .long("network")
                .value_name("HOST:PORT")
                .takes_value(true)
                .help("Rendezvous with the cluster at this gossip entry point"),
        )
        .arg(
            Arg::with_name("no_leader_rotation")
                .long("no-leader-rotation")
                .help("Disable leader rotation"),
        )
        .arg(
            Arg::with_name("no_signer")
                .long("no-signer")
                .takes_value(false)
                .conflicts_with("signer")
                .help("Launch node without vote signer"),
        )
        .arg(
            Arg::with_name("no_sigverify")
                .short("v")
                .long("no-sigverify")
                .takes_value(false)
                .help("Run without signature verification"),
        )
        .arg(
            Arg::with_name("rpc_port")
                .long("rpc-port")
                .value_name("PORT")
                .takes_value(true)
                .help("RPC port to use for this node"),
        )
        .arg(
            Arg::with_name("enable_rpc_exit")
                .long("enable-rpc-exit")
                .takes_value(false)
                .help("Enable the JSON RPC 'fullnodeExit' API.  Only enable in a debug environment"),
        )
        .arg(
            Arg::with_name("rpc_drone_address")
                .long("rpc-drone-address")
                .value_name("HOST:PORT")
                .takes_value(true)
                .help("Enable the JSON RPC 'requestAirdrop' API with this drone address."),
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
            Arg::with_name("accounts")
                .short("a")
                .long("accounts")
                .value_name("PATHS")
                .takes_value(true)
                .help("Comma separated persistent accounts location"),
        )
        .arg(
            clap::Arg::with_name("public_address")
                .long("public-address")
                .takes_value(false)
                .help("Advertise public machine address in gossip.  By default the local machine address is advertised"),
        )
        .arg(
            clap::Arg::with_name("gossip_port")
                .long("gossip-port")
                .value_name("PORT")
                .takes_value(true)
                .help("Gossip port number for the node"),
        )
        .get_matches();

    let mut fullnode_config = FullnodeConfig::default();
    let keypair = if let Some(identity) = matches.value_of("identity") {
        read_keypair(identity).unwrap_or_else(|err| {
            eprintln!("{}: Unable to open keypair file: {}", err, identity);
            exit(1);
        })
    } else {
        Keypair::new()
    };
    let ledger_path = matches.value_of("ledger").unwrap();

    fullnode_config.sigverify_disabled = matches.is_present("no_sigverify");
    let no_signer = matches.is_present("no_signer");
    fullnode_config.voting_disabled = no_signer;
    let use_only_bootstrap_leader = matches.is_present("no_leader_rotation");

    if matches.is_present("enable_rpc_exit") {
        fullnode_config.rpc_config.enable_fullnode_exit = true;
    }
    fullnode_config.rpc_config.drone_addr = matches
        .value_of("rpc_drone_address")
        .map(|address| address.parse().expect("failed to parse drone address"));

    let gossip_addr = {
        let mut addr = solana_netutil::parse_port_or_addr(
            matches.value_of("gossip_port"),
            FULLNODE_PORT_RANGE.0 + 1,
        );
        if matches.is_present("public_address") {
            addr.set_ip(solana_netutil::get_public_ip_addr().unwrap());
        } else {
            addr.set_ip(solana_netutil::get_ip_addr(false).unwrap());
        }
        addr
    };

    if let Some(paths) = matches.value_of("accounts") {
        fullnode_config.account_paths = Some(paths.to_string());
    } else {
        fullnode_config.account_paths = None;
    }
    let cluster_entrypoint = matches.value_of("network").map(|network| {
        let gossip_addr = network.parse().expect("failed to parse network address");
        NodeInfo::new_entry_point(&gossip_addr)
    });
    let (_signer_service, signer_addr) = if let Some(signer_addr) = matches.value_of("signer") {
        (
            None,
            signer_addr.to_string().parse().expect("Signer IP Address"),
        )
    } else {
        // Run a local vote signer if a vote signer service address was not provided
        let (signer_service, signer_addr) = LocalVoteSignerService::new();
        (Some(signer_service), signer_addr)
    };
    let (rpc_port, rpc_pubsub_port) = if let Some(port) = matches.value_of("rpc_port") {
        let port_number = port.to_string().parse().expect("integer");
        if port_number == 0 {
            eprintln!("Invalid RPC port requested: {:?}", port);
            exit(1);
        }
        (port_number, port_number + 1)
    } else {
        (
            solana_netutil::find_available_port_in_range(FULLNODE_PORT_RANGE)
                .expect("unable to allocate rpc_port"),
            solana_netutil::find_available_port_in_range(FULLNODE_PORT_RANGE)
                .expect("unable to allocate rpc_pubsub_port"),
        )
    };
    let init_complete_file = matches.value_of("init_complete_file");
    fullnode_config.blockstream = matches.value_of("blockstream").map(|s| s.to_string());

    let keypair = Arc::new(keypair);
    let mut node = Node::new_with_external_ip(keypair.pubkey(), &gossip_addr);
    node.info.rpc.set_port(rpc_port);
    node.info.rpc_pubsub.set_port(rpc_pubsub_port);

    let genesis_block = GenesisBlock::load(ledger_path).expect("Unable to load genesis block");
    if use_only_bootstrap_leader && node.info.id != genesis_block.bootstrap_leader_id {
        fullnode_config.voting_disabled = true;
    }

    let vote_signer: Box<dyn VoteSigner + Sync + Send> = if !no_signer {
        info!("Vote signer service address: {:?}", signer_addr);
        Box::new(RemoteVoteSigner::new(signer_addr))
    } else {
        info!("Node will not vote");
        Box::new(LocalVoteSigner::default())
    };
    let vote_signer = VotingKeypair::new_with_signer(&keypair, vote_signer);
    let vote_account_id = vote_signer.pubkey();
    info!("New vote account ID is {:?}", vote_account_id);

    let gossip_addr = node.info.gossip;
    let fullnode = Fullnode::new(
        node,
        &keypair,
        ledger_path,
        vote_signer,
        cluster_entrypoint.as_ref(),
        &fullnode_config,
    );

    if !fullnode_config.voting_disabled {
        let leader_node_info = loop {
            info!("Looking for leader...");
            match poll_gossip_for_leader(gossip_addr, Some(10)) {
                Ok(leader_node_info) => {
                    info!("Found leader: {:?}", leader_node_info);
                    break leader_node_info;
                }
                Err(err) => {
                    info!("Unable to find leader: {:?}", err);
                }
            };
        };

        let mut client = mk_client(&leader_node_info);
        if let Err(err) = create_and_fund_vote_account(&mut client, vote_account_id, &keypair) {
            panic!("Failed to create_and_fund_vote_account: {:?}", err);
        }
    }

    if let Some(filename) = init_complete_file {
        File::create(filename).unwrap_or_else(|_| panic!("Unable to create: {}", filename));
    }
    info!("Node initialized");
    fullnode.join().expect("fullnode exit");
    info!("Node exiting..");
}
