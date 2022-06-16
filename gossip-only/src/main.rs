#![allow(clippy::integer_arithmetic)]
#[cfg(not(target_env = "msvc"))]
use {
    log::*,
    solana_gossip::{
        cluster_info::{
            ClusterInfo, Node,
        },
        gossip_service::GossipService,
    },
    solana_net_utils::VALIDATOR_PORT_RANGE,
    solana_runtime::{
        accounts_db,
        accounts_index::AccountSecondaryIndexes,
        bank::Bank,
        bank_forks::BankForks,
        genesis_utils::create_genesis_config_with_leader,
    },
    crossbeam_channel::unbounded,
    std::{
        sync::{
            atomic::AtomicBool,
            Arc, RwLock,
        },
        net::{SocketAddr,IpAddr},
        str::FromStr,
    },
    solana_streamer::socket::SocketAddrSpace,
    solana_sdk::signature::{Keypair, Signer},
    tempfile::TempDir,
};


pub fn main() {
    solana_logger::setup_with_default("solana=info");

    let default_dynamic_port_range =
        &format!("{}-{}", VALIDATOR_PORT_RANGE.0, VALIDATOR_PORT_RANGE.1);
    
    let dynamic_port_range =
        solana_net_utils::parse_port_range(default_dynamic_port_range)
            .expect("invalid dynamic_port_range");
            
    let socket_addr_space = SocketAddrSpace::Unspecified;
    let exit = Arc::new(AtomicBool::new(false));

    let identity_keypair = Keypair::new();
    let gossip_addr = "127.0.0.1:8001";
    let gossip_addr : SocketAddr = gossip_addr
        .parse()
        .expect("Unable to parse socket address");

    info!("gossip_addr: {:?}", gossip_addr);
    let bind_address = IpAddr::from_str("0.0.0.0").unwrap();

    let node = Node::new_with_external_ip(
        &identity_keypair.pubkey(),
        &gossip_addr,
        dynamic_port_range,
        bind_address,
        None,
    );

    let identity_keypair = Arc::new(identity_keypair);
    let cluster_info = ClusterInfo::new(
        node.info.clone(),
        identity_keypair.clone(),
        socket_addr_space,
    );


    cluster_info.set_entrypoints(vec![]);
    let cluster_info = Arc::new(cluster_info);
    let (stats_reporter_sender, _stats_reporter_receiver) = unbounded();

    let accounts_dir = TempDir::new().unwrap();
    let genesis_config_info = create_genesis_config_with_leader(
        10_000,                          // mint_lamports
        &solana_sdk::pubkey::new_rand(), // validator_pubkey
        1,                               // validator_stake_lamports
    );

    //Generate bank forks
    let bank0 = Bank::new_with_paths_for_tests(
        &genesis_config_info.genesis_config,
        vec![accounts_dir.path().to_path_buf()],
        None,
        None,
        AccountSecondaryIndexes::default(),
        false,
        accounts_db::AccountShrinkThreshold::default(),
        false,
    );
    bank0.freeze();
    let bank_forks = BankForks::new(bank0);
    let bank_forks = Arc::new(RwLock::new(bank_forks));
   
    // Run Gossip
    let gossip_service = GossipService::new(
        &cluster_info,
        Some(bank_forks.clone()),
        node.sockets.gossip,
        None,
        true,   //should check dup instance
        Some(stats_reporter_sender.clone()),

        &exit,
    );
    gossip_service.join().unwrap();



}








