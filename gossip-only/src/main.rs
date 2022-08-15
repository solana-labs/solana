#![allow(clippy::integer_arithmetic)]

#[cfg(not(target_env = "msvc"))]
use {
    clap::{
        crate_description, crate_name, value_t, value_t_or_exit, values_t, App, Arg, ArgMatches,
    },
    crossbeam_channel::unbounded,
    log::*,
    solana_clap_utils::input_validators::is_port,
    solana_core::gen_keys::GenKeys,
    solana_gossip::{
        cluster_info::{ClusterInfo, Node},
        contact_info::ContactInfo,
        gossip_service::GossipService,
    },
    solana_net_utils::VALIDATOR_PORT_RANGE,
    solana_runtime::{
        accounts_db, accounts_index::AccountSecondaryIndexes, bank::Bank, bank_forks::BankForks,
        genesis_utils::create_genesis_config_with_leader, runtime_config::RuntimeConfig,
    },
    solana_sdk::{
        native_token::LAMPORTS_PER_SOL,
        signature::{Keypair, Signer},
    },
    solana_streamer::socket::SocketAddrSpace,
    std::{
        collections::HashMap,
        collections::HashSet,
        env,
        fs::{self, File},
        io::Write,
        net::{IpAddr, Ipv4Addr, SocketAddr},
        path::Path,
        process::exit,
        str::FromStr,
        sync::{atomic::AtomicBool, Arc, RwLock},
    },
    tempfile::TempDir,
};

pub const DEFAULT_CLUSTER_LAMPORTS: u64 = 10_000_000 * LAMPORTS_PER_SOL;
pub const DEFAULT_NODE_STAKE: u64 = 10 * LAMPORTS_PER_SOL;
// pub const DEFAULT_SHRED_VERSION: u16 = 0;

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
        .arg(
            Arg::with_name("write_keys")
                .long("write-keys")
                .value_name("KEYS")
                .takes_value(false)
                .help("set if you just want to write keys to a file"),
        )
        .arg(
            Arg::with_name("num_keys")
                .long("num-keys")
                .value_name("KEYS")
                .takes_value(true)
                .help("Number of keys to write"),
        )
        .arg(
            Arg::with_name("num_nodes")
                .long("num-nodes")
                .value_name("NODES")
                .takes_value(true)
                .help("Number of nodes to spin up"),
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
            clap::Arg::with_name("gossip_port")
                .long("gossip-port")
                .value_name("PORT")
                .takes_value(true)
                .validator(is_port)
                .help("Gossip port number for the node"),
        )
        .arg(
            Arg::with_name("gossip_host")
                .long("gossip-host")
                .value_name("HOST")
                .takes_value(true)
                .validator(solana_net_utils::is_host)
                .help(
                    "Gossip DNS name or IP address for the node to advertise in gossip \
                       [default: ask --entrypoint, or 127.0.0.1 when --entrypoint is not provided]",
                ),
        )
        .arg(
            Arg::with_name("bootstrap")
                .long("bootstrap")
                .value_name("BOOTSTRAP")
                .takes_value(false)
                .help("set to run bootstrap validator"),
        )
        .get_matches()
}

fn parse_entrypoint(matches: &ArgMatches) -> Option<SocketAddr> {
    matches.value_of("entrypoint").map(|entrypoint| {
        solana_net_utils::parse_host_port(entrypoint).unwrap_or_else(|e| {
            eprintln!("failed to parse entrypoint address: {}", e);
            exit(1);
        })
    })
}

pub fn generate_keypairs(seed_keypair: &Keypair, count: u64) -> (Vec<Keypair>, u64) {
    let mut seed = [0u8; 32];
    seed.copy_from_slice(&seed_keypair.to_bytes()[..32]);
    let mut rnd = GenKeys::new(seed);

    (rnd.gen_n_keypairs(count), 0)
}

fn parse_gossip_host(matches: &ArgMatches, entrypoint_addr: Option<SocketAddr>) -> IpAddr {
    matches
        .value_of("gossip_host")
        .map(|gossip_host| {
            solana_net_utils::parse_host(gossip_host).unwrap_or_else(|e| {
                eprintln!("failed to parse gossip-host: {}", e);
                exit(1);
            })
        })
        .unwrap_or_else(|| {
            if let Some(entrypoint_addr) = entrypoint_addr {
                solana_net_utils::get_public_ip_addr(&entrypoint_addr).unwrap_or_else(|err| {
                    eprintln!(
                        "Failed to contact cluster entrypoint {}: {}",
                        entrypoint_addr, err
                    );
                    exit(1);
                })
            } else {
                IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1))
            }
        })
}

pub fn main() {
    solana_logger::setup_with_default("info");
    let matches = parse_matches();
    let account_infile = value_t_or_exit!(matches, "account_file", String);
    let write_keys = matches.is_present("write_keys");

    //Write keys to file and exit
    if write_keys {
        let keypair_count = value_t!(matches, "num_keys", u64).unwrap_or_else(|_| {
            error!("Number of keys to write was not specified! Use \"--num-keys <count>\"");
            exit(1);
        });

        info!("Generating {} keypairs", keypair_count);
        let id = &Keypair::new();
        let (keypairs, _) = generate_keypairs(id, keypair_count);
        let mut accounts = HashMap::new();
        keypairs.iter().for_each(|keypair| {
            accounts.insert(
                serde_json::to_string(&keypair.to_bytes().to_vec()).unwrap(),
                DEFAULT_NODE_STAKE, // this value can be a struct if we want. like Base64Account
            );
        });
        info!("Writing {}", account_infile);
        let serialized = serde_yaml::to_string(&accounts).unwrap();
        let path = Path::new(&account_infile);
        let mut file = File::create(path).unwrap();
        file.write_all(&serialized.into_bytes()).unwrap();

        info!("Wrote {} keys to file: {}", keypair_count, account_infile);
        exit(0);
    }

    let shred_version =
        value_t!(matches, "shred_version", u16).unwrap_or_else(|_| 0);

    // Read keys from file and spin up gossip nodes
    let bind_address = IpAddr::from_str("0.0.0.0").unwrap();
    let cluster_lamports = DEFAULT_CLUSTER_LAMPORTS;
    let socket_addr_space = SocketAddrSpace::Unspecified;

    //Set dynamic port range for node ports
    let default_dynamic_port_range =
        &format!("{}-{}", VALIDATOR_PORT_RANGE.0, VALIDATOR_PORT_RANGE.1);
    let dynamic_port_range = solana_net_utils::parse_port_range(default_dynamic_port_range)
        .expect("invalid dynamic_port_range");

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
    println!("entrypoint_addr: {:?}", entrypoint_addrs);

    // READ ACCOUNTS FROM FILE
    let path = Path::new(&account_infile);
    let file = File::open(path).unwrap();

    info!("Reading {}", account_infile);
    let accounts: HashMap<String, u64> = serde_yaml::from_reader(file).unwrap();
    info!("{} accounts read in", accounts.len());

    let num_nodes = value_t!(matches, "num_nodes", usize).unwrap_or_else(|_| {
        error!("Number of gossip nodes to spin up was not specified! Use \"--num-nodes <count>\"");
        exit(1);
    });
    if num_nodes > accounts.len() {
        error!("Attempting to spin up more nodes than there are accounts...exiting...");
        exit(1);
    }

    info!(
        "Spinning up {} Gossip nodes with the following accounts/stakes: ",
        num_nodes
    );
    let mut count = 0;
    for (k, v) in &accounts {
        println!("{}, {}", k, v);
        count = count + 1;
        if count == num_nodes {
            break;
        }
    }

    let mut node_keys: Vec<Keypair> = Vec::new();
    let mut node_stakes: Vec<u64> = Vec::new();
    accounts.into_iter().for_each(|(keypair, stake)| {
        let bytes: Vec<u8> = serde_json::from_str(keypair.as_str()).unwrap();
        node_keys.push(Keypair::from_bytes(&bytes).unwrap());
        node_stakes.push(stake);
    });

    //Setup genesis config.
    let genesis_config_info = &mut create_genesis_config_with_leader(
        cluster_lamports,
        &node_keys[0].pubkey(),
        node_stakes[0],
    );
    let genesis_config = &mut genesis_config_info.genesis_config;

    let accounts_dir = TempDir::new().unwrap();
    let ledger_path = accounts_dir.path().to_path_buf();
    let ledger_path = fs::canonicalize(&ledger_path).unwrap_or_else(|err| {
        eprintln!("Unable to access ledger path: {:?}", err);
        exit(1);
    });

    let mut gossip_threads: Vec<GossipService> = Vec::new();
    let entrypoint_addr = parse_entrypoint(&matches);
    let gossip_host = parse_gossip_host(&matches, entrypoint_addr);
    let gossip_addr = SocketAddr::new(
        gossip_host,
        value_t!(matches, "gossip_port", u16).unwrap_or_else(|_| {
            solana_net_utils::find_available_port_in_range(
                IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)),
                (0, 1),
            )
            .expect("unable to find an available gossip port")
        }),
    );

    //set entrypoints for gossip
    let cluster_entrypoints = entrypoint_addrs
        .iter()
        .map(ContactInfo::new_gossip_entry_point)
        .collect::<Vec<_>>();

    let is_bootstrap = matches.is_present("bootstrap");
    //Run bootstrapi gossip-node
    if is_bootstrap {
        //create new node with external ip
        let mut node = Node::new_with_external_ip(
            &node_keys[0].pubkey(),
            &gossip_addr,
            dynamic_port_range,
            bind_address,
            None,
        );
        let my_keypair = Arc::new(Keypair::from_bytes(&node_keys[0].to_bytes()).unwrap());
        node.info.shred_version = shred_version;

        let cluster_info =
            ClusterInfo::new(node.info.clone(), my_keypair.clone(), socket_addr_space);

        cluster_info.set_entrypoints(cluster_entrypoints.clone());
        let cluster_info = Arc::new(cluster_info);

        //Generate new bank and bank forks
        let bank0 = Bank::new_with_paths_for_tests(
            genesis_config,
            Arc::<RuntimeConfig>::default(),
            vec![ledger_path.clone()],
            AccountSecondaryIndexes::default(),
            false,
            accounts_db::AccountShrinkThreshold::default(),
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
            node.sockets.gossip,
            None,
            true, //should check dup instance
            Some(stats_reporter_sender.clone()),
            &exit_gossip,
        );

        gossip_threads.push(gossip_service);
    } else {
        // Loop through nodes, spin up a leader first, then have all others join leader
        for i in 0..num_nodes {
            //create new node with external ip
            let mut node = Node::new_with_external_ip(
                &node_keys[i].pubkey(),
                &gossip_addr,
                dynamic_port_range,
                bind_address,
                None,
            );
            let my_keypair = Arc::new(Keypair::from_bytes(&node_keys[i].to_bytes()).unwrap());
            node.info.shred_version = shred_version;

            let cluster_info =
                ClusterInfo::new(node.info.clone(), my_keypair.clone(), socket_addr_space);

            cluster_info.set_entrypoints(cluster_entrypoints.clone());
            let cluster_info = Arc::new(cluster_info);

            let (stats_reporter_sender, _stats_reporter_receiver) = unbounded();

            // Run Gossip
            let exit_gossip = Arc::new(AtomicBool::new(false));
            let gossip_service = GossipService::new(
                &cluster_info,
                // Some(bank_forks.clone()),
                None,
                node.sockets.gossip,
                None,
                true, //should check dup instance
                Some(stats_reporter_sender.clone()),
                &exit_gossip,
            );

            gossip_threads.push(gossip_service);
        }
    }

    for thread in gossip_threads {
        thread.join().unwrap();
    }
}
