#![allow(clippy::integer_arithmetic)]
#[cfg(not(target_env = "msvc"))]
// use jemallocator::Jemalloc;
///Try calling: make_gossip_node() in gossip_service.rs

// extern crate log;
use {
    clap::{
        crate_description, crate_name, value_t, value_t_or_exit, App, AppSettings, Arg, ArgMatches,
        SubCommand,
    },
    solana_clap_utils::input_validators::{is_keypair_or_ask_keyword, is_port, is_pubkey},
    log::*,
    solana_gossip::{
        cluster_info::{
            ClusterInfo, Node, //DEFAULT_CONTACT_DEBUG_INTERVAL_MILLIS,
            // DEFAULT_CONTACT_SAVE_INTERVAL_MILLIS,
        },
        // contact_info::ContactInfo,
        // crds_gossip_pull::CRDS_GOSSIP_PULL_CRDS_TIMEOUT_MS,
        gossip_service::GossipService,
    },
    solana_runtime::{
        accounts_db::{self, ACCOUNTS_DB_CONFIG_FOR_TESTING},
        accounts_index::AccountSecondaryIndexes,
        bank::Bank,
        bank_forks::BankForks,
        genesis_utils::{create_genesis_config_with_leader, GenesisConfigInfo},
    },
    crossbeam_channel::{unbounded, Receiver},
    std::{
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc, RwLock,
        }
    },
    solana_streamer::socket::SocketAddrSpace,
    solana_sdk::signature::Keypair,
    tempfile::TempDir,
};


pub fn main() {
    // env_logger::init();
    solana_logger::setup_with_default("solana=info");
    // let socket_addr_space = SocketAddrSpace::new(matches.is_present("allow_private_addr"));
    let socket_addr_space = SocketAddrSpace::Unspecified;
    let exit = Arc::new(AtomicBool::new(false));

    let identity_keypair = Arc::new(Keypair::new());
    let node = Node::new_localhost();
    let mut cluster_info = ClusterInfo::new(
        node.info.clone(),
        identity_keypair.clone(),
        socket_addr_space,
    );


    // cluster_info.set_contact_debug_interval(config.contact_debug_interval);
    cluster_info.set_entrypoints(vec![]);
    // cluster_info.restore_contact_info(ledger_path, config.contact_save_interval);
    let cluster_info = Arc::new(cluster_info);
    let (stats_reporter_sender, _stats_reporter_receiver) = unbounded();

    let accounts_dir = TempDir::new().unwrap();
    let mut genesis_config_info = create_genesis_config_with_leader(
        10_000,                          // mint_lamports
        &solana_sdk::pubkey::new_rand(), // validator_pubkey
        1,                               // validator_stake_lamports
    );
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
    // let bank_forks = Arc::new(bank_forks);

    

    // let (crossbeam_channel::Sender<<Box<dyn FnOnce() + Send>> stats_reporter_sender, _stats_reporter_receiver) = unbounded();
   
    let gossip_service = GossipService::new(
        &cluster_info,
        Some(bank_forks.clone()),
        node.sockets.gossip,
        // config.gossip_validators.clone(),
        None,
        true,   //should check dup instance
        Some(stats_reporter_sender.clone()),
        // None,

        &exit,
    );
    gossip_service.join().unwrap();



}








