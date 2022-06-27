#![allow(clippy::integer_arithmetic)]

#[cfg(not(target_env = "msvc"))]
use {
    log::*,
    clap::{
        crate_description, crate_name, value_t, values_t, App, Arg, ArgMatches,
    },
    solana_core::{
        validator::{ValidatorConfig},
    },
    solana_gossip::{
        cluster_info::{
            ClusterInfo, Node,
        },
        gossip_service::GossipService,
        contact_info::ContactInfo,
    },
    rand::{seq::SliceRandom, thread_rng},
    solana_net_utils::VALIDATOR_PORT_RANGE,
    solana_runtime::{
        accounts_db,
        accounts_index::AccountSecondaryIndexes,
        bank::Bank,
        bank_forks::BankForks,
        genesis_utils::{
            create_genesis_config_with_vote_accounts_and_cluster_type,
            create_genesis_config_with_leader,
            ValidatorVoteKeypairs,
        },
    },
    crossbeam_channel::unbounded,
    std::{
        sync::{
            atomic::AtomicBool,
            Arc, RwLock,
        },
        net::{SocketAddr,IpAddr, Ipv4Addr},
        str::FromStr,
        process::exit,
        collections::HashSet,
        iter,

    },
    solana_local_cluster::{
        local_cluster::ClusterConfig,
        validator_configs::make_identical_validator_configs,
    },
    solana_sdk::{
        signature::{Signer, Keypair},
        native_token::LAMPORTS_PER_SOL,
    },
    solana_streamer::socket::SocketAddrSpace,
    tempfile::TempDir,

};

pub const DEFAULT_CLUSTER_LAMPORTS: u64 = 10_000_000 * LAMPORTS_PER_SOL;
pub const DEFAULT_NODE_STAKE: u64 = 10 * LAMPORTS_PER_SOL;

pub fn main() {
    solana_logger::setup_with_default("info");
    let exit_gossip = Arc::new(AtomicBool::new(false));
    let bind_address = IpAddr::from_str("0.0.0.0").unwrap();
    let cluster_lamports = DEFAULT_CLUSTER_LAMPORTS;
    let socket_addr_space = SocketAddrSpace::Unspecified;

    //Set dynamic port range for node ports
    let default_dynamic_port_range =
        &format!("{}-{}", VALIDATOR_PORT_RANGE.0, VALIDATOR_PORT_RANGE.1);
    let dynamic_port_range =
        solana_net_utils::parse_port_range(default_dynamic_port_range)
            .expect("invalid dynamic_port_range");



    info!("suhhh");

    let priv_node_key: &[u8] = &[232, 216, 7, 12, 130, 87, 248, 13, 41, 47, 182, 219, 230, 218, 35, 24, 
                                65, 87, 42, 211, 220, 253, 31, 144, 91, 236, 82, 86, 147, 124, 207, 190, 
                                166, 88, 56, 42, 28, 22, 140, 124, 46, 146, 5, 112, 155, 180, 33, 96, 
                                43, 38, 97, 68, 150, 25, 54, 135, 146, 41, 243, 84, 91, 156, 188, 133];

    let priv_vote_key: &[u8] = &[202, 35, 250, 0, 21, 185, 178, 195, 95, 109, 163, 124, 249, 63, 82, 196, 
                                154, 15, 247, 26, 127, 15, 41, 162, 97, 252, 18, 33, 166, 14, 88, 18, 37, 
                                147, 252, 70, 228, 141, 254, 161, 251, 179, 207, 232, 54, 226, 190, 223, 
                                77, 17, 229, 79, 193, 210, 131, 85, 231, 169, 0, 162, 22, 93, 90, 24];

    let priv_stake_key: &[u8] = &[121, 154, 185, 120, 121, 119, 208, 158, 154, 183, 96, 104, 87, 79, 191, 
                                165, 63, 20, 14, 148, 221, 196, 216, 66, 107, 110, 214, 125, 102, 38, 5, 
                                92, 114, 72, 21, 228, 214, 107, 206, 150, 75, 47, 110, 206, 203, 71, 220, 
                                164, 238, 82, 204, 75, 100, 86, 106, 26, 78, 2, 46, 90, 204, 224, 21, 230];

    let stake: u64 = 10 * LAMPORTS_PER_SOL; 

    let node_keypair = Keypair::from_bytes(&priv_node_key)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string())).unwrap();
    let vote_keypair = Keypair::from_bytes(&priv_vote_key)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string())).unwrap();
    let stake_keypair = Keypair::from_bytes(&priv_stake_key)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string())).unwrap();


    let mut node_keys: Vec<Keypair> = Vec::new();
    let mut vote_keys: Vec<Keypair> = Vec::new();
    let mut stake_keys: Vec<Keypair> = Vec::new();
    let mut stakes_in_genesis: Vec<u64> = Vec::new();

    node_keys.push(node_keypair);
    vote_keys.push(vote_keypair);
    stake_keys.push(stake_keypair);
    stakes_in_genesis.push(stake);

    // let(keys_in_genesis, stakes_in_genesis): (Vec<ValidatorVoteKeypairs>, Vec<u64>) = 
    //     node_keys
    //         .iter()
    //         .zip(&vote_keys.iter())
    //         .zip(&stake_keys.iter());
    
    // let keys_in_genesis: Vec<ValidatorVoteKeypairs> = 
    //     node_keys
    //         .iter()
    //         .zip(&vote_keys)
    //         .zip(&stake_keys)
    //         .filter_map(|(node_keypair, vote_keypair, stake_keypair)| {
    //             info!("what upppppp, {},{},{}", node_keypair, vote_keypair, stake_keypair);
    //         })
    //         .unzip();

    // let leader_keypair = &keys_in_genesis[0].node_keypair;
    let leader_keypair = &node_keys[0];
    let leader_pubkey = leader_keypair.pubkey();

    //Entrypoint to join Gossip Cluster
    let entrypoint_addrs: Vec<SocketAddr> = Vec::new();
    // let entrypoint = "127.0.0.1:8001";
    // entrypoint_addrs.push(
    //     entrypoint
    //         .parse()
    //         .expect("Unable to parse socket address")
    // );
    
    let gossip_host = IpAddr::from_str("127.0.0.1").unwrap();
    // let gossip_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8001);
    let gossip_addr = SocketAddr::new(
        gossip_host,
        solana_net_utils::find_available_port_in_range(bind_address, (0, 1)).unwrap_or_else(
            |err| {
                eprintln!("Unable to find an available gossip port: {}", err);
                exit(1);
            }
        )
    );

        // value_t!(matches, "gossip_port", u16).unwrap_or_else(|_| {
        //     solana_net_utils::find_available_port_in_range(bind_address, (0, 1)).unwrap_or_else(
        //         |err| {
        //             eprintln!("Unable to find an available gossip port: {}", err);
        //             exit(1);
        //         },
        //     )
        // }),
    // );

    //create new node with external ip
    let leader_node = Node::new_with_external_ip(
        &leader_pubkey,
        &gossip_addr,
        dynamic_port_range,
        bind_address,
        None,
    );
    let leader_keypair = Arc::new(Keypair::from_bytes(&leader_keypair.to_bytes()).unwrap());

    let num_nodes = node_keys.len();
    let config = ClusterConfig {
        node_stakes: stakes_in_genesis,
        cluster_lamports,
        validator_configs: make_identical_validator_configs(
            &ValidatorConfig::default_for_test(),
            num_nodes,
        ),
        ..ClusterConfig::default()
    };

    //Setup genesis config.
    let genesis_config_info = &mut create_genesis_config_with_leader(
        config.cluster_lamports,
        &leader_pubkey,
        config.node_stakes[0],
    );


    // let genesis_config_info = &mut create_genesis_config_with_vote_accounts_and_cluster_type(
    //     config.cluster_lamports,
    //     &keys_in_genesis,
    //     stakes_in_genesis,
    //     config.cluster_type,
    // );

    let genesis_config = &mut genesis_config_info.genesis_config;

    let cluster_info = ClusterInfo::new(
        leader_node.info.clone(),
        leader_keypair.clone(),
        socket_addr_space,
    );

    //set entrypoints for gossip
    let cluster_entrypoints = entrypoint_addrs
        .iter()
        .map(ContactInfo::new_gossip_entry_point)
        .collect::<Vec<_>>();

    cluster_info.set_entrypoints(cluster_entrypoints);
    let cluster_info = Arc::new(cluster_info);

    //Generate new bank and bank forks
    let accounts_dir = TempDir::new().unwrap();
    let bank0 = Bank::new_with_paths_for_tests(
        genesis_config,
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

    let (stats_reporter_sender, _stats_reporter_receiver) = unbounded();

    // Run Gossip
    let gossip_service = GossipService::new(
        &cluster_info,
        Some(bank_forks.clone()),
        leader_node.sockets.gossip,
        None,
        true,   //should check dup instance
        Some(stats_reporter_sender.clone()),

        &exit_gossip,
    );
    gossip_service.join().unwrap();

}
