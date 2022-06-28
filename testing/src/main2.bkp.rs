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
            ValidatorVoteKeypairs,
        },
    },
    crossbeam_channel::unbounded,
    std::{
        sync::{
            atomic::AtomicBool,
            Arc, RwLock,
        },
        net::{SocketAddr,IpAddr},
        str::FromStr,
        process::exit,
        collections::HashSet,
        iter,

    },
    solana_local_cluster::{
        local_cluster::ClusterConfig,
        validator_configs::make_identical_validator_configs,
    },
    solana_streamer::socket::SocketAddrSpace,
    solana_sdk::{
        signature::{Keypair, Signer},
        native_token::LAMPORTS_PER_SOL,
    },
    tempfile::TempDir,
};

pub const DEFAULT_CLUSTER_LAMPORTS: u64 = 10_000_000 * LAMPORTS_PER_SOL;
pub const DEFAULT_NODE_STAKE: u64 = 10 * LAMPORTS_PER_SOL;

fn parse_matches() -> ArgMatches<'static> {
    // let shred_version_arg = Arg::with_name("shred_version")
    //     .long("shred-version")
    //     .value_name("VERSION")
    //     .takes_value(true)
    //     .default_value("0")
    //     .help("Filter gossip nodes by this shred version");

    App::new(crate_name!())
        .about(crate_description!())
        .version(solana_version::version!())
        .arg(
            Arg::with_name("gossip_host")
                .long("gossip-host")
                .value_name("HOST")
                .takes_value(true)
                .validator(solana_net_utils::is_host)
                .help("Gossip DNS name or IP address for the validator to advertise in gossip \
                       [default: ask --entrypoint, or 127.0.0.1 when --entrypoint is not provided]"),
        )
        .arg(
            Arg::with_name("gossip_port")
                .long("gossip-port")
                .value_name("PORT")
                .takes_value(true)
                .help("Gossip port number for the validator"),
        )
        .arg(
            Arg::with_name("entrypoint")
                .short("n")
                .long("entrypoint")
                .value_name("HOST:PORT")
                .takes_value(true)
                .validator(solana_net_utils::is_host_port)
                .help("Rendezvous with the cluster at this entrypoint"),
        )
        .arg(
            Arg::with_name("num_nodes")
                .long("num-nodes")
                .value_name("NODES")
                .takes_value(true)
                .default_value("1")
                .help("Number of gossip nodes in cluster"),
        )
        .arg(
            Arg::with_name("node_stake")
                .short("stake")
                .long("node-stake")
                .value_name("STAKE")
                .takes_value(true)
                .help("Stake of node. unit: lamports"),
        )
        .get_matches()
}

pub fn main() {
    solana_logger::setup_with_default("info");
    let matches = parse_matches();
    let socket_addr_space = SocketAddrSpace::Unspecified;
    let exit_gossip = Arc::new(AtomicBool::new(false));
    let cluster_lamports = DEFAULT_CLUSTER_LAMPORTS;
    let bind_address = IpAddr::from_str("0.0.0.0").unwrap();

    //Set dynamic port range for node ports
    let default_dynamic_port_range =
        &format!("{}-{}", VALIDATOR_PORT_RANGE.0, VALIDATOR_PORT_RANGE.1);
    let dynamic_port_range =
        solana_net_utils::parse_port_range(default_dynamic_port_range)
            .expect("invalid dynamic_port_range");

    // Get node stake from command line. Default node stake if not passed in
    let node_stake = value_t!(matches, "node_stake", u64).unwrap_or_else(|_| {
        DEFAULT_NODE_STAKE
    });

    //Entrypoint to join Gossip Cluster
    let entrypoint_addrs = values_t!(matches, "entrypoint", String)
        .unwrap_or_default()
        .into_iter()
        .map(|entrypoint| {
            solana_net_utils::parse_host_port(&entrypoint).unwrap_or_else(|e| {
                eprintln!("failed to parse entrypoint address: {}", e);
                exit(1);
            })
        })
        .collect::<HashSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();

    //set gossip IP. will find public IP if one not specified
    let gossip_host: IpAddr = matches
        .value_of("gossip_host")
        .map(|gossip_host| {
            solana_net_utils::parse_host(gossip_host).unwrap_or_else(|err| {
                eprintln!("Failed to parse --gossip-host: {}", err);
                exit(1);
            })
        })
        .unwrap_or_else(|| {
            if !entrypoint_addrs.is_empty() {
                let mut order: Vec<_> = (0..entrypoint_addrs.len()).collect();
                order.shuffle(&mut thread_rng());

                let gossip_host = order.into_iter().find_map(|i| {
                    let entrypoint_addr = &entrypoint_addrs[i];
                    info!(
                        "Contacting {} to determine the validator's public IP address",
                        entrypoint_addr
                    );
                    solana_net_utils::get_public_ip_addr(entrypoint_addr).map_or_else(
                        |err| {
                            eprintln!(
                                "Failed to contact cluster entrypoint {}: {}",
                                entrypoint_addr, err
                            );
                            None
                        },
                        Some,
                    )
                });

                gossip_host.unwrap_or_else(|| {
                    eprintln!("Unable to determine the validator's public IP address");
                    exit(1);
                })
            } else {
                std::net::IpAddr::V4(std::net::Ipv4Addr::new(127, 0, 0, 1))
            }
        });
    
    //set gossip addr (IP:port). Gets gossip port from commandline or finds available port
    let gossip_addr = SocketAddr::new(
        gossip_host,
        value_t!(matches, "gossip_port", u16).unwrap_or_else(|_| {
            solana_net_utils::find_available_port_in_range(bind_address, (0, 1)).unwrap_or_else(
                |err| {
                    eprintln!("Unable to find an available gossip port: {}", err);
                    exit(1);
                },
            )
        }),
    );
   
    //Setup cluster with Node stakes. 
    //Note we're only creating one node here since we're not creating a local cluster
    let num_nodes = 1;
    let stakes: Vec<_> = (0..num_nodes).map(|_| node_stake).collect();
    let config = ClusterConfig {
        node_stakes: stakes,
        cluster_lamports,
        validator_configs: make_identical_validator_configs(
            &ValidatorConfig::default_for_test(),
            num_nodes,
        ),
        ..ClusterConfig::default()
    };
    assert_eq!(config.validator_configs.len(), config.node_stakes.len());

    // let priv_node_key: &[u8] = &[232, 216, 7, 12, 130, 87, 248, 13, 41, 47, 182, 219, 230, 218, 35, 24, 
    //                             65, 87, 42, 211, 220, 253, 31, 144, 91, 236, 82, 86, 147, 124, 207, 190, 
    //                             166, 88, 56, 42, 28, 22, 140, 124, 46, 146, 5, 112, 155, 180, 33, 96, 
    //                             43, 38, 97, 68, 150, 25, 54, 135, 146, 41, 243, 84, 91, 156, 188, 133];

    let priv_node_key: &[u8] = &[121, 154, 185, 120, 121, 119, 208, 158, 154, 183, 96, 104, 87, 79, 191, 
                                165, 63, 20, 14, 148, 221, 196, 216, 66, 107, 110, 214, 125, 102, 38, 5, 
                                92, 114, 72, 21, 228, 214, 107, 206, 150, 75, 47, 110, 206, 203, 71, 220, 
                                164, 238, 82, 204, 75, 100, 86, 106, 26, 78, 2, 46, 90, 204, 224, 21, 230];
    // let node_keypair = Keypair::from_bytes(&priv_node_key)
    //     .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string())).unwrap();   
    

    //generate keys for validators (aka just one validator)
    // let mut validator_keys: Vec<(Arc<Keypair>, bool)> = iter::repeat_with(|| (Arc::new(Keypair::new()), false))
    //     .take(config.validator_configs.len())
    //     .collect();

    let mut validator_keys: Vec<(Arc<Keypair>, bool)> = iter::repeat_with(|| (Arc::new(Keypair::from_bytes(&priv_node_key).unwrap()), false))
        .take(config.validator_configs.len())
        .collect();


    // let mut validator_keys: Vec<(Arc<Keypair>, bool)> = Vec::new();
    // let node_keypair = Arc::new(node_keypair);
    // validator_keys.push((node_keypair, true));

    // generate vote keys for a node/validator? (Just one)
    // Not sure why we need diff vote keys
    let vote_keys: Vec<Arc<Keypair>> = iter::repeat_with(|| Arc::new(Keypair::new()))
        .take(config.validator_configs.len())
        .collect();

    // Bootstrap leader should always be in genesis block
    // Think we need whoever starting cluster to run this part. aka include their key in genesis block
    validator_keys[0].1 = true;
    let (keys_in_genesis, stakes_in_genesis): (Vec<ValidatorVoteKeypairs>, Vec<u64>) =
        validator_keys
            .iter()
            .zip(&config.node_stakes)
            .zip(&vote_keys)
            .filter_map(|(((node_keypair, in_genesis), stake), vote_keypair)| {
                info!(
                    "STARTING LOCAL CLUSTER: key {} vote_key {} has {} stake",
                    node_keypair.pubkey(),
                    vote_keypair.pubkey(),
                    stake
                );
                if *in_genesis {
                    Some((
                        ValidatorVoteKeypairs {
                            node_keypair: Keypair::from_bytes(&node_keypair.to_bytes())
                                .unwrap(),
                            vote_keypair: Keypair::from_bytes(&vote_keypair.to_bytes())
                                .unwrap(),
                            stake_keypair: Keypair::new(),
                        },
                        stake,
                    ))
                } else {
                    None
                }
            })
            .unzip();

    let leader_keypair = &keys_in_genesis[0].node_keypair;
    let leader_pubkey = leader_keypair.pubkey();

    //create new node with external ip
    let leader_node = Node::new_with_external_ip(
        &leader_pubkey,
        &gossip_addr,
        dynamic_port_range,
        bind_address,
        None,
    );
    let leader_keypair = Arc::new(Keypair::from_bytes(&leader_keypair.to_bytes()).unwrap());

    //Setup genesis config.
    let genesis_config_info = &mut create_genesis_config_with_vote_accounts_and_cluster_type(
        config.cluster_lamports,
        &keys_in_genesis,
        stakes_in_genesis,
        config.cluster_type,
    );

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








