#![allow(clippy::integer_arithmetic)]
#[cfg(not(target_env = "msvc"))]
use {
    log::*,
    clap::{
        crate_description, crate_name, value_t, value_t_or_exit, values_t, App, AppSettings, Arg, ArgMatches,
        SubCommand,
    },
    solana_gossip::{
        cluster_info::{
            ClusterInfo, Node,
        },
        gossip_service::GossipService,
        contact_info::ContactInfo,
    },
    solana_net_utils::{self, VALIDATOR_PORT_RANGE},
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
        collections::HashSet,
        process::exit,
    },
    solana_streamer::socket::SocketAddrSpace,
    solana_sdk::signature::{Keypair, Signer},
    tempfile::TempDir,
};

fn parse_matches() -> ArgMatches<'static> {
    let shred_version_arg = Arg::with_name("shred_version")
        .long("shred-version")
        .value_name("VERSION")
        .takes_value(true)
        .default_value("0")
        .help("Filter gossip nodes by this shred version");

    App::new(crate_name!())
        .about(crate_description!())
        .version(solana_version::version!())
        .arg(
            Arg::with_name("entrypoint")
                .short("n")
                .long("entrypoint")
                .value_name("HOST:PORT")
                .takes_value(true)
                .validator(solana_net_utils::is_host_port)
                .help("Rendezvous with the cluster at this entrypoint"),
        )
        .get_matches()
}


pub fn main() {
    solana_logger::setup_with_default("solana=info");
    let matches = parse_matches();

    let default_dynamic_port_range =
        &format!("{}-{}", VALIDATOR_PORT_RANGE.0, VALIDATOR_PORT_RANGE.1);
    
    let dynamic_port_range =
        solana_net_utils::parse_port_range(default_dynamic_port_range)
            .expect("invalid dynamic_port_range");
            
    let socket_addr_space = SocketAddrSpace::Unspecified;
    let exit_gossip = Arc::new(AtomicBool::new(false));

    let identity_keypair = Keypair::new();
    let gossip_addr = "127.0.0.1:8100";
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

    for addr in &entrypoint_addrs {
        if !socket_addr_space.check(addr) {
            eprintln!("invalid entrypoint address: {}", addr);
            exit(1);
        }
    }


    let cluster_entrypoints = entrypoint_addrs
        .iter()
        .map(ContactInfo::new_gossip_entry_point)
        .collect::<Vec<_>>();


    // cluster_info.set_entrypoints(vec![]);
    cluster_info.set_entrypoints(cluster_entrypoints);
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

        &exit_gossip,
    );
    gossip_service.join().unwrap();



}








