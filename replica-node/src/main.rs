//! The main AccountsDb replication node responsible for replicating
//! AccountsDb information from peer a validator or another replica-node.

#![allow(clippy::integer_arithmetic)]

use {
    clap::{crate_description, crate_name, value_t, values_t, App, AppSettings, Arg},
    log::*,
    rand::{seq::SliceRandom, thread_rng},
    solana_clap_utils::{
        input_parsers::keypair_of,
        input_validators::{is_keypair_or_ask_keyword, is_parsable, is_pubkey},
        keypair::SKIP_SEED_PHRASE_VALIDATION_ARG,
    },
    solana_gossip::{
        cluster_info::{Node, VALIDATOR_PORT_RANGE},
        contact_info::ContactInfo,
    },
    solana_replica_node::{
        replica_node::{ReplicaNode, ReplicaNodeConfig},
        replica_util,
    },
    solana_rpc::{rpc::JsonRpcConfig, rpc_pubsub_service::PubSubConfig},
    solana_runtime::accounts_index::AccountSecondaryIndexes,
    solana_sdk::{exit::Exit, pubkey::Pubkey, signature::Signer},
    solana_streamer::socket::SocketAddrSpace,
    solana_validator::port_range_validator,
    std::{
        collections::HashSet,
        env,
        net::{IpAddr, SocketAddr},
        path::PathBuf,
        process::exit,
        sync::{Arc, RwLock},
    },
};

pub fn main() {
    let default_dynamic_port_range =
        &format!("{}-{}", VALIDATOR_PORT_RANGE.0, VALIDATOR_PORT_RANGE.1);

    let matches = App::new(crate_name!())
        .about(crate_description!())
        .version(solana_version::version!())
        .setting(AppSettings::VersionlessSubcommands)
        .setting(AppSettings::InferSubcommands)
        .arg(
            Arg::with_name(SKIP_SEED_PHRASE_VALIDATION_ARG.name)
                .long(SKIP_SEED_PHRASE_VALIDATION_ARG.long)
                .help(SKIP_SEED_PHRASE_VALIDATION_ARG.help),
        )
        .arg(
            Arg::with_name("ledger_path")
                .short("l")
                .long("ledger")
                .value_name("DIR")
                .takes_value(true)
                .required(true)
                .default_value("ledger")
                .help("Use DIR as ledger location"),
        )
        .arg(
            Arg::with_name("snapshots")
                .long("snapshots")
                .value_name("DIR")
                .takes_value(true)
                .help("Use DIR as snapshot location [default: --ledger value]"),
        )
        .arg(
            Arg::with_name("peer_address")
                .long("peer-address")
                .value_name("IP")
                .takes_value(true)
                .required(true)
                .help("The the address for the peer validator/replica to download from"),
        )
        .arg(
            Arg::with_name("peer_rpc_port")
                .long("peer-rpc-port")
                .value_name("PORT")
                .takes_value(true)
                .required(true)
                .help("The the PORT for the peer validator/replica from which to download the snapshots"),
        )
        .arg(
            Arg::with_name("peer_accountsdb_repl_port")
                .long("peer-accountsdb-repl-port")
                .value_name("PORT")
                .takes_value(true)
                .required(true)
                .help("The the PORT for the peer validator/replica serving the AccountsDb replication"),
        )
        .arg(
            Arg::with_name("peer_pubkey")
                .long("peer-pubkey")
                .validator(is_pubkey)
                .value_name("The peer validator/replica IDENTITY")
                .required(true)
                .takes_value(true)
                .help("The pubkey for the target validator."),
        )
        .arg(
            Arg::with_name("account_paths")
                .long("accounts")
                .value_name("PATHS")
                .takes_value(true)
                .multiple(true)
                .help("Comma separated persistent accounts location"),
        )
        .arg(
            Arg::with_name("identity")
                .short("i")
                .long("identity")
                .value_name("KEYPAIR")
                .takes_value(true)
                .validator(is_keypair_or_ask_keyword)
                .help("Replica identity keypair"),
        )
        .arg(
            Arg::with_name("entrypoint")
                .short("n")
                .long("entrypoint")
                .value_name("HOST:PORT")
                .takes_value(true)
                .multiple(true)
                .validator(solana_net_utils::is_host_port)
                .help("Rendezvous with the cluster at this gossip entrypoint"),
        )
        .arg(
            Arg::with_name("bind_address")
                .long("bind-address")
                .value_name("HOST")
                .takes_value(true)
                .validator(solana_net_utils::is_host)
                .default_value("0.0.0.0")
                .help("IP address to bind the replica ports"),
        )
        .arg(
            Arg::with_name("rpc_bind_address")
                .long("rpc-bind-address")
                .value_name("HOST")
                .takes_value(true)
                .validator(solana_net_utils::is_host)
                .help("IP address to bind the Json RPC port [default: use --bind-address]"),
        )
        .arg(
            Arg::with_name("rpc_port")
                .long("rpc-port")
                .value_name("PORT")
                .takes_value(true)
                .validator(solana_validator::port_validator)
                .help("Enable JSON RPC on this port, and the next port for the RPC websocket"),
        )
        .arg(
            Arg::with_name("dynamic_port_range")
                .long("dynamic-port-range")
                .value_name("MIN_PORT-MAX_PORT")
                .takes_value(true)
                .default_value(default_dynamic_port_range)
                .validator(port_range_validator)
                .help("Range to use for dynamically assigned ports"),
        )
        .arg(
            Arg::with_name("expected_shred_version")
                .long("expected-shred-version")
                .value_name("VERSION")
                .takes_value(true)
                .validator(is_parsable::<u16>)
                .help("Require the shred version be this value"),
        )
        .arg(
            Arg::with_name("logfile")
                .short("o")
                .long("log")
                .value_name("FILE")
                .takes_value(true)
                .help(
                    "Redirect logging to the specified file, '-' for standard error. \
                       Sending the SIGUSR1 signal to the validator process will cause it \
                       to re-open the log file",
                ),
        )
        .arg(
            Arg::with_name("allow_private_addr")
                .long("allow-private-addr")
                .takes_value(false)
                .help("Allow contacting private ip addresses")
                .hidden(true),
        )
        .get_matches();

    let bind_address = solana_net_utils::parse_host(matches.value_of("bind_address").unwrap())
        .expect("invalid bind_address");

    let rpc_bind_address = if let Some(rpc_bind_address) = matches.value_of("rpc_bind_address") {
        solana_net_utils::parse_host(rpc_bind_address).expect("invalid rpc_bind_address")
    } else {
        bind_address
    };

    let identity_keypair = keypair_of(&matches, "identity").unwrap_or_else(|| {
        clap::Error::with_description(
            "The --identity <KEYPAIR> argument is required",
            clap::ErrorKind::ArgumentNotFound,
        )
        .exit();
    });

    let peer_pubkey = value_t!(matches, "peer_pubkey", Pubkey).unwrap();

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

    let expected_shred_version = value_t!(matches, "expected_shred_version", u16)
        .ok()
        .or_else(|| replica_util::get_cluster_shred_version(&entrypoint_addrs));

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

    let dynamic_port_range =
        solana_net_utils::parse_port_range(matches.value_of("dynamic_port_range").unwrap())
            .expect("invalid dynamic_port_range");

    let cluster_entrypoints = entrypoint_addrs
        .iter()
        .map(ContactInfo::new_gossip_entry_point)
        .collect::<Vec<_>>();

    let node = Node::new_with_external_ip(
        &identity_keypair.pubkey(),
        &gossip_addr,
        dynamic_port_range,
        bind_address,
    );

    let ledger_path = PathBuf::from(matches.value_of("ledger_path").unwrap());
    let snapshot_archives_dir = if let Some(snapshots) = matches.value_of("snapshots") {
        PathBuf::from(snapshots)
    } else {
        ledger_path.clone()
    };
    let bank_snapshots_dir = snapshot_archives_dir.join("snapshot");

    let account_paths: Vec<PathBuf> =
        if let Ok(account_paths) = values_t!(matches, "account_paths", String) {
            account_paths
                .join(",")
                .split(',')
                .map(PathBuf::from)
                .collect()
        } else {
            vec![ledger_path.join("accounts")]
        };

    let peer_address = solana_net_utils::parse_host(matches.value_of("peer_address").unwrap())
        .expect("invalid peer_address");

    let peer_rpc_port = value_t!(matches, "peer_rpc_port", u16).unwrap_or_else(|_| {
        clap::Error::with_description(
            "The --peer-rpc-port <PORT> argument is required",
            clap::ErrorKind::ArgumentNotFound,
        )
        .exit();
    });

    let rpc_peer_addr = SocketAddr::new(peer_address, peer_rpc_port);

    let peer_accountsdb_repl_port = value_t!(matches, "peer_accountsdb_repl_port", u16)
        .unwrap_or_else(|_| {
            clap::Error::with_description(
                "The --peer-accountsdb-repl-port <PORT> argument is required",
                clap::ErrorKind::ArgumentNotFound,
            )
            .exit();
        });

    let accountsdb_repl_peer_addr = SocketAddr::new(peer_address, peer_accountsdb_repl_port);

    let rpc_port = value_t!(matches, "rpc_port", u16).unwrap_or_else(|_| {
        clap::Error::with_description(
            "The --rpc-port <PORT> argument is required",
            clap::ErrorKind::ArgumentNotFound,
        )
        .exit();
    });
    let rpc_addrs = (
        SocketAddr::new(rpc_bind_address, rpc_port),
        SocketAddr::new(rpc_bind_address, rpc_port + 1),
        // If additional ports are added, +2 needs to be skipped to avoid a conflict with
        // the websocket port (which is +2) in web3.js This odd port shifting is tracked at
        // https://github.com/solana-labs/solana/issues/12250
    );

    let logfile = {
        let logfile = matches
            .value_of("logfile")
            .map(|s| s.into())
            .unwrap_or_else(|| format!("solana-replica-node-{}.log", identity_keypair.pubkey()));

        if logfile == "-" {
            None
        } else {
            println!("log file: {}", logfile);
            Some(logfile)
        }
    };
    let socket_addr_space = SocketAddrSpace::new(matches.is_present("allow_private_addr"));

    let _logger_thread = solana_validator::redirect_stderr_to_file(logfile);

    let (cluster_info, rpc_contact_info, snapshot_info) = replica_util::get_rpc_peer_info(
        identity_keypair,
        &cluster_entrypoints,
        &ledger_path,
        &node,
        expected_shred_version,
        &peer_pubkey,
        &snapshot_archives_dir,
        socket_addr_space,
    );

    info!(
        "Using RPC service from node {}: {:?}, snapshot_info: {:?}",
        rpc_contact_info.id, rpc_contact_info.rpc, snapshot_info
    );

    let config = ReplicaNodeConfig {
        rpc_peer_addr,
        accountsdb_repl_peer_addr: Some(accountsdb_repl_peer_addr),
        rpc_addr: rpc_addrs.0,
        rpc_pubsub_addr: rpc_addrs.1,
        ledger_path,
        snapshot_archives_dir,
        bank_snapshots_dir,
        account_paths,
        snapshot_info: snapshot_info.unwrap(),
        cluster_info,
        rpc_config: JsonRpcConfig::default(),
        snapshot_config: None,
        pubsub_config: PubSubConfig::default(),
        socket_addr_space,
        account_indexes: AccountSecondaryIndexes::default(),
        accounts_db_caching_enabled: false,
        replica_exit: Arc::new(RwLock::new(Exit::default())),
    };

    let replica = ReplicaNode::new(config);
    replica.join();
}
