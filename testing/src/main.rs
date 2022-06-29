#![allow(clippy::integer_arithmetic)]
#[cfg(not(target_env = "msvc"))]
use {
    log::*,
    clap::{
        crate_description, crate_name, value_t, value_t_or_exit, App, Arg, ArgMatches,
    },
    solana_core::{
        validator::{ValidatorConfig},
        gen_keys::GenKeys,
    },
    solana_gossip::{
        cluster_info::{
            ClusterInfo, Node,
        },
        gossip_service::GossipService,
        contact_info::ContactInfo,
    },
    solana_net_utils::VALIDATOR_PORT_RANGE,
    solana_runtime::{
        accounts_db,
        accounts_index::AccountSecondaryIndexes,
        bank::Bank,
        bank_forks::BankForks,
        genesis_utils::{
            create_genesis_config_with_leader,
        },
    },
    crossbeam_channel::unbounded,
    std::{
        sync::{
            atomic::AtomicBool,
            Arc, RwLock,
        },
        path::Path,
        net::{SocketAddr,IpAddr, Ipv4Addr},
        str::FromStr,
        process::exit,
        env,
        fs::File,
        collections::HashMap,
        io::Write,
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
pub const DEFAULT_SHRED_VERSION: u16 = 44120;

pub fn generate_keypairs(seed_keypair: &Keypair, count: u64) -> (Vec<Keypair>, u64) {
    let mut seed = [0u8; 32];
    seed.copy_from_slice(&seed_keypair.to_bytes()[..32]);
    let mut rnd = GenKeys::new(seed);

    (rnd.gen_n_keypairs(count), 0)
}

fn parse_matches() -> ArgMatches<'static> {

    App::new(crate_name!())
        .about(crate_description!())
        .version(solana_version::version!())
        .arg(
            Arg::with_name("shred_version")
                .long("shred-version")
                .value_name("VERSION")
                .takes_value(true)
                .help("Filter gossip nodes by this shred version"),
        )
        .arg(
            Arg::with_name("account_file")
                .long("account-file")
                .value_name("PATH")
                .takes_value(true)
                .help("CSV File with Accounts"),
        )
        .get_matches()
}

pub fn main() {
    solana_logger::setup_with_default("info");
    let matches = parse_matches();
    let bind_address = IpAddr::from_str("0.0.0.0").unwrap();
    let cluster_lamports = DEFAULT_CLUSTER_LAMPORTS;
    let socket_addr_space = SocketAddrSpace::Unspecified;

    let account_infile = value_t_or_exit!(matches, "account_file", String);

    // WRITE ACCOUNTS TO FILE
    let keypair_count = 3;
    info!("Generating {} keypairs", keypair_count);
    let id = &Keypair::new();
    let (keypairs, _) = generate_keypairs(id, keypair_count as u64);
    let mut accounts = HashMap::new();
    keypairs.iter().for_each(|keypair| {
        accounts.insert(
            serde_json::to_string(&keypair.to_bytes().to_vec()).unwrap(),
            DEFAULT_NODE_STAKE,         // this value can be a struct if we want. like Base64Account
        );
    });
    info!("Writing {}", account_infile);
    let serialized = serde_yaml::to_string(&accounts).unwrap();
    let path = Path::new(&account_infile);
    let mut file = File::create(path).unwrap();
    file.write_all(&serialized.into_bytes()).unwrap();


    // READ ACCOUNTS FROM FILE
    let path = Path::new(&account_infile);
    let file = File::open(path).unwrap();

    info!("Reading {}", account_infile);
    let accounts: HashMap<String, u64> = serde_yaml::from_reader(file).unwrap();

    info!("Accounts read in:");
    for (k,v) in &accounts {
        println!("{}, {}", k, v);
    }

    let mut node_keys: Vec<Keypair> = Vec::new();
    let mut node_stakes: Vec<u64> = Vec::new();

    accounts
        .into_iter()
        .for_each(|(keypair, stake)| {
            let bytes: Vec<u8> = serde_json::from_str(keypair.as_str()).unwrap();
            node_keys.push(Keypair::from_bytes(&bytes).unwrap());
            node_stakes.push(stake);
        });

    //Set dynamic port range for node ports
    let default_dynamic_port_range =
        &format!("{}-{}", VALIDATOR_PORT_RANGE.0, VALIDATOR_PORT_RANGE.1);
    let dynamic_port_range =
        solana_net_utils::parse_port_range(default_dynamic_port_range)
            .expect("invalid dynamic_port_range");


    let shred_version = value_t!(matches, "shred_version", u16).unwrap_or_else(|_| {
        DEFAULT_SHRED_VERSION
    });


    let leader_keypair = &node_keys[0];
    let leader_pubkey = leader_keypair.pubkey();

    let num_nodes = node_keys.len();

    let mut config = ClusterConfig::default();

    //Setup genesis config.
    let genesis_config_info = &mut create_genesis_config_with_leader(
        cluster_lamports,
        &leader_pubkey,
        node_stakes[0],
    );
    let genesis_config = &mut genesis_config_info.genesis_config;

    let mut gossip_thread_vector: Vec<GossipService> = Vec::new();

    for i in 0..num_nodes {
        let mut entrypoint_addrs: Vec<SocketAddr> = Vec::new();
        let gossip_host = IpAddr::from_str("127.0.0.1").unwrap();
        if i == 0 {
            //Entrypoint to join Gossip Cluster
            let gossip_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8001);

            //create new node with external ip
            let mut leader_node = Node::new_with_external_ip(   
                &leader_pubkey,
                &gossip_addr,
                dynamic_port_range,
                bind_address,
                None,
            );
            let leader_keypair = Arc::new(Keypair::from_bytes(&leader_keypair.to_bytes()).unwrap());
            leader_node.info.shred_version = shred_version;
        

            config.node_stakes = node_stakes.clone();
            config.cluster_lamports = cluster_lamports;
            config.validator_configs = make_identical_validator_configs(&ValidatorConfig::default_for_test(), 1);

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
            let exit_gossip = Arc::new(AtomicBool::new(false));
            let gossip_service = GossipService::new(
                &cluster_info,
                Some(Arc::clone(&bank_forks.clone())),
                leader_node.sockets.gossip,
                None,
                true,   //should check dup instance
                Some(stats_reporter_sender.clone()),

                &exit_gossip,
            );

            gossip_thread_vector.push(gossip_service);
        } 
        else {
            let entrypoint = "127.0.0.1:8001";
            entrypoint_addrs.push(
                entrypoint
                .parse()
                .expect("Unable to parse socket address")
            );
            let gossip_addr = SocketAddr::new(
                gossip_host,
                solana_net_utils::find_available_port_in_range(bind_address, (0, 1)).unwrap_or_else(
                    |err| {
                        eprintln!("Unable to find an available gossip port: {}", err);
                        exit(1);
                    }
                )
            );

            //create new node with external ip
            let mut my_node = Node::new_with_external_ip( //run this with pubkey[i]
                &node_keys[i].pubkey(),
                &gossip_addr,
                dynamic_port_range,
                bind_address,
                None,
            );
            let my_keypair = Arc::new(Keypair::from_bytes(&node_keys[i].to_bytes()).unwrap());
            my_node.info.shred_version = shred_version;

            let cluster_info = ClusterInfo::new(
                my_node.info.clone(),
                my_keypair.clone(),
                socket_addr_space,
            );

            //set entrypoints for gossip
            let cluster_entrypoints = entrypoint_addrs
                .iter()
                .map(ContactInfo::new_gossip_entry_point)
                .collect::<Vec<_>>();

            cluster_info.set_entrypoints(cluster_entrypoints);
            // cluster_info.my_contact_info().
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
            let exit_gossip = Arc::new(AtomicBool::new(false));
            let gossip_service = GossipService::new(
                &cluster_info,
                Some(bank_forks.clone()),
                my_node.sockets.gossip,
                None,
                true,   //should check dup instance
                Some(stats_reporter_sender.clone()),

                &exit_gossip,
            );

            gossip_thread_vector.push(gossip_service);
        }
    }
    for thread in gossip_thread_vector {
        thread.join().unwrap();
    }

}
