#![allow(clippy::integer_arithmetic)]
#[cfg(not(target_env = "msvc"))]
use {
    log::*,
    clap::{
        crate_description, crate_name, value_t, values_t, value_t_or_exit, App, Arg, ArgMatches,
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
            create_genesis_config_with_leader, 
            create_genesis_config_with_vote_accounts, 
            ValidatorVoteKeypairs
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
    },
    solana_streamer::socket::SocketAddrSpace,
    solana_sdk::signature::{Keypair, Signer},
    tempfile::TempDir,
};

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
        .get_matches()
}

pub fn main() {
    solana_logger::setup_with_default("solana=info,solana::gossip-only=info");
    // solana_logger::setup(");

    error!("greg_hey");
    let matches = parse_matches();

    let default_dynamic_port_range =
        &format!("{}-{}", VALIDATOR_PORT_RANGE.0, VALIDATOR_PORT_RANGE.1);
    
    let dynamic_port_range =
        solana_net_utils::parse_port_range(default_dynamic_port_range)
            .expect("invalid dynamic_port_range");
            
    let socket_addr_space = SocketAddrSpace::Unspecified;
    let exit_gossip = Arc::new(AtomicBool::new(false));

    let identity_keypair = Keypair::new();
    let bind_address = IpAddr::from_str("0.0.0.0").unwrap();

    //Get Gossip Entry Point if passed in 
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

    //Setup Gossip IP
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

    //Setup Gossip IP:Port
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

    info!("gossip_addr: {:?}", gossip_addr);

    //Setup Node
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

    let cluster_entrypoints = entrypoint_addrs
        .iter()
        .map(ContactInfo::new_gossip_entry_point)
        .collect::<Vec<_>>();

    cluster_info.set_entrypoints(cluster_entrypoints);
    let cluster_info = Arc::new(cluster_info);
    let (stats_reporter_sender, _stats_reporter_receiver) = unbounded();

    let accounts_dir = TempDir::new().unwrap();
    // let genesis_config_info = create_genesis_config_with_leader(
    //     10_000,                          // mint_lamports
    //     &solana_sdk::pubkey::new_rand(), // validator_pubkey
    //     1,                               // validator_stake_lamports
    // );

    let num_nodes: usize = value_t_or_exit!(matches, "num_nodes", usize);

    // let num_nodes: usize = std::env::var("NUM_NODES")
    //     .unwrap_or_else(|_| "10".to_string())
    //     .parse()
    //     .expect("could not parse NUM_NODES as a number");

    info!("greg_num_nodes: {}", num_nodes);
    let vote_keypairs: Vec<_> = (0..num_nodes)
        .map(|_| ValidatorVoteKeypairs::new_rand())
        .collect();
    let genesis_config_info = create_genesis_config_with_vote_accounts(
        10_000,
        &vote_keypairs,
        vec![100; vote_keypairs.len()],
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

        &exit_gossip,
    );
    gossip_service.join().unwrap();



}








