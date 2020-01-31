use bzip2::bufread::BzDecoder;
use clap::{
    crate_description, crate_name, value_t, value_t_or_exit, values_t_or_exit, App, Arg, ArgMatches,
};
use console::{style, Emoji};
use indicatif::{ProgressBar, ProgressStyle};
use log::*;
use rand::{thread_rng, Rng};
use solana_clap_utils::{
    input_parsers::pubkey_of,
    input_validators::{is_keypair, is_pubkey_or_keypair},
    keypair::{
        self, keypair_input, KeypairWithSource, ASK_SEED_PHRASE_ARG,
        SKIP_SEED_PHRASE_VALIDATION_ARG,
    },
};
use solana_client::rpc_client::RpcClient;
use solana_core::ledger_cleanup_service::DEFAULT_MAX_LEDGER_SLOTS;
use solana_core::{
    cluster_info::{ClusterInfo, Node, VALIDATOR_PORT_RANGE},
    contact_info::ContactInfo,
    gossip_service::GossipService,
    rpc::JsonRpcConfig,
    validator::{Validator, ValidatorConfig},
};
use solana_ledger::bank_forks::SnapshotConfig;
use solana_perf::recycler::enable_recycler_warming;
use solana_sdk::{
    clock::Slot,
    hash::Hash,
    pubkey::Pubkey,
    signature::{Keypair, KeypairUtil},
};
use std::{
    fs::{self, File},
    io::{self, Read},
    net::{SocketAddr, TcpListener},
    path::{Path, PathBuf},
    process::exit,
    str::FromStr,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, RwLock,
    },
    thread::sleep,
    time::{Duration, Instant},
};

fn port_validator(port: String) -> Result<(), String> {
    port.parse::<u16>()
        .map(|_| ())
        .map_err(|e| format!("{:?}", e))
}

fn port_range_validator(port_range: String) -> Result<(), String> {
    if solana_net_utils::parse_port_range(&port_range).is_some() {
        Ok(())
    } else {
        Err("Invalid port range".to_string())
    }
}

fn hash_validator(hash: String) -> Result<(), String> {
    Hash::from_str(&hash)
        .map(|_| ())
        .map_err(|e| format!("{:?}", e))
}

static TRUCK: Emoji = Emoji("🚚 ", "");
static SPARKLE: Emoji = Emoji("✨ ", "");

/// Creates a new process bar for processing that will take an unknown amount of time
fn new_spinner_progress_bar() -> ProgressBar {
    let progress_bar = ProgressBar::new(42);
    progress_bar
        .set_style(ProgressStyle::default_spinner().template("{spinner:.green} {wide_msg}"));
    progress_bar.enable_steady_tick(100);
    progress_bar
}

fn download_tar_bz2(
    rpc_addr: &SocketAddr,
    archive_name: &str,
    download_path: &Path,
    is_snapshot: bool,
) -> Result<(), String> {
    let archive_path = download_path.join(archive_name);
    if archive_path.is_file() {
        return Ok(());
    }
    fs::create_dir_all(download_path).map_err(|err| err.to_string())?;

    let temp_archive_path = {
        let mut p = archive_path.clone();
        p.set_extension(".tmp");
        p
    };

    let url = format!("http://{}/{}", rpc_addr, archive_name);
    let download_start = Instant::now();

    let progress_bar = new_spinner_progress_bar();
    progress_bar.set_message(&format!("{}Downloading {}...", TRUCK, url));

    let client = reqwest::blocking::Client::new();
    let response = client
        .get(url.as_str())
        .send()
        .map_err(|err| err.to_string())?;

    if is_snapshot && response.status() == reqwest::StatusCode::NOT_FOUND {
        progress_bar.finish_and_clear();
        warn!("Snapshot not found at {}", url);
        return Ok(());
    }

    let response = response
        .error_for_status()
        .map_err(|err| format!("Unable to get: {:?}", err))?;
    let download_size = {
        response
            .headers()
            .get(reqwest::header::CONTENT_LENGTH)
            .and_then(|content_length| content_length.to_str().ok())
            .and_then(|content_length| content_length.parse().ok())
            .unwrap_or(0)
    };
    progress_bar.set_length(download_size);
    progress_bar.set_style(
        ProgressStyle::default_bar()
            .template(&format!(
                "{}{}Downloading {} {}",
                "{spinner:.green} ",
                TRUCK,
                url,
                "[{bar:40.cyan/blue}] {bytes}/{total_bytes} ({eta})"
            ))
            .progress_chars("=> "),
    );

    struct DownloadProgress<R> {
        progress_bar: ProgressBar,
        response: R,
    }

    impl<R: Read> Read for DownloadProgress<R> {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            self.response.read(buf).map(|n| {
                self.progress_bar.inc(n as u64);
                n
            })
        }
    }

    let mut source = DownloadProgress {
        progress_bar,
        response,
    };

    let mut file = File::create(&temp_archive_path)
        .map_err(|err| format!("Unable to create {:?}: {:?}", temp_archive_path, err))?;
    std::io::copy(&mut source, &mut file)
        .map_err(|err| format!("Unable to write {:?}: {:?}", temp_archive_path, err))?;

    source.progress_bar.finish_and_clear();
    info!(
        "  {}{}",
        SPARKLE,
        format!(
            "Downloaded {} ({} bytes) in {:?}",
            url,
            download_size,
            Instant::now().duration_since(download_start),
        )
    );

    if !is_snapshot {
        info!("Extracting {:?}...", archive_path);
        let extract_start = Instant::now();
        let tar_bz2 = File::open(&temp_archive_path)
            .map_err(|err| format!("Unable to open {}: {:?}", archive_name, err))?;
        let tar = BzDecoder::new(std::io::BufReader::new(tar_bz2));
        let mut archive = tar::Archive::new(tar);
        archive
            .unpack(download_path)
            .map_err(|err| format!("Unable to unpack {}: {:?}", archive_name, err))?;
        info!(
            "Extracted {} in {:?}",
            archive_name,
            Instant::now().duration_since(extract_start)
        );
    }
    std::fs::rename(temp_archive_path, archive_path)
        .map_err(|err| format!("Unable to rename: {:?}", err))?;

    Ok(())
}

fn get_rpc_addr(
    node: &Node,
    identity_keypair: &Arc<Keypair>,
    entrypoint_gossip: &SocketAddr,
    expected_shred_version: Option<u16>,
) -> (RpcClient, SocketAddr) {
    let mut cluster_info = ClusterInfo::new(
        ClusterInfo::spy_contact_info(&identity_keypair.pubkey()),
        identity_keypair.clone(),
    );
    cluster_info.set_entrypoint(ContactInfo::new_gossip_entry_point(entrypoint_gossip));
    let cluster_info = Arc::new(RwLock::new(cluster_info));

    let gossip_exit_flag = Arc::new(AtomicBool::new(false));
    let gossip_service = GossipService::new(
        &cluster_info.clone(),
        None,
        node.sockets.gossip.try_clone().unwrap(),
        &gossip_exit_flag,
    );

    let (rpc_client, rpc_addr) = loop {
        info!(
            "Searching for RPC service, shred version={:?}...\n{}",
            expected_shred_version,
            cluster_info.read().unwrap().contact_info_trace()
        );

        let mut rpc_peers = cluster_info.read().unwrap().all_rpc_peers();

        let shred_version_required = !rpc_peers
            .iter()
            .all(|contact_info| contact_info.shred_version == rpc_peers[0].shred_version);

        if let Some(expected_shred_version) = expected_shred_version {
            // Filter out rpc peers that don't match the expected shred version
            rpc_peers = rpc_peers
                .into_iter()
                .filter(|contact_info| contact_info.shred_version == expected_shred_version)
                .collect::<Vec<_>>();
        }

        if !rpc_peers.is_empty() {
            // Prefer the entrypoint's RPC service if present, otherwise pick a node at random
            let contact_info = if let Some(contact_info) = rpc_peers
                .iter()
                .find(|contact_info| contact_info.gossip == *entrypoint_gossip)
            {
                Some(contact_info.clone())
            } else if shred_version_required && expected_shred_version.is_none() {
                // Require the user supply a shred version if there are conflicting shred version in
                // gossip to reduce the chance of human error
                warn!("Multiple shred versions in gossip.  Restart with --expected-shred-version");
                None
            } else {
                // Pick a node at random
                Some(rpc_peers[thread_rng().gen_range(0, rpc_peers.len())].clone())
            };

            if let Some(ContactInfo { id, rpc, .. }) = contact_info {
                info!("Contacting RPC port of node {}: {:?}", id, rpc);
                let rpc_client = RpcClient::new_socket(rpc);
                match rpc_client.get_version() {
                    Ok(rpc_version) => {
                        info!("RPC node version: {}", rpc_version.solana_core);
                        break (rpc_client, rpc);
                    }
                    Err(err) => {
                        warn!("Failed to get RPC version: {}", err);
                    }
                }
            }
        } else {
            info!("No RPC service found");
        }

        sleep(Duration::from_secs(1));
    };

    gossip_exit_flag.store(true, Ordering::Relaxed);
    gossip_service.join().unwrap();

    (rpc_client, rpc_addr)
}

fn check_vote_account(
    rpc_client: &RpcClient,
    vote_pubkey: &Pubkey,
    voting_pubkey: &Pubkey,
    node_pubkey: &Pubkey,
) -> Result<(), String> {
    let found_vote_account = rpc_client
        .get_account(vote_pubkey)
        .map_err(|err| format!("Failed to get vote account: {}", err.to_string()))?;

    if found_vote_account.owner != solana_vote_program::id() {
        return Err(format!(
            "not a vote account (owned by {}): {}",
            found_vote_account.owner, vote_pubkey
        ));
    }

    let found_node_account = rpc_client
        .get_account(node_pubkey)
        .map_err(|err| format!("Failed to get identity account: {}", err.to_string()))?;

    let found_vote_account = solana_vote_program::vote_state::VoteState::from(&found_vote_account);
    if let Some(found_vote_account) = found_vote_account {
        if found_vote_account.authorized_voter != *voting_pubkey {
            return Err(format!(
                "account's authorized voter ({}) does not match to the given voting keypair ({}).",
                found_vote_account.authorized_voter, voting_pubkey
            ));
        }
        if found_vote_account.node_pubkey != *node_pubkey {
            return Err(format!(
                "account's node pubkey ({}) does not match to the given identity keypair ({}).",
                found_vote_account.node_pubkey, node_pubkey
            ));
        }
    } else {
        return Err(format!("invalid vote account data: {}", vote_pubkey));
    }

    // Maybe we can calculate minimum voting fee; rather than 1 lamport
    if found_node_account.lamports <= 1 {
        return Err(format!(
            "unfunded identity account ({}): only {} lamports (needs more fund to vote)",
            node_pubkey, found_node_account.lamports
        ));
    }

    Ok(())
}

fn download_ledger(
    rpc_addr: &SocketAddr,
    ledger_path: &Path,
    no_snapshot_fetch: bool,
) -> Result<(), String> {
    download_tar_bz2(rpc_addr, "genesis.tar.bz2", ledger_path, false)?;

    if !no_snapshot_fetch {
        let snapshot_package =
            solana_ledger::snapshot_utils::get_snapshot_archive_path(ledger_path);
        if snapshot_package.exists() {
            fs::remove_file(&snapshot_package)
                .map_err(|err| format!("error removing {:?}: {}", snapshot_package, err))?;
        }
        download_tar_bz2(
            rpc_addr,
            snapshot_package.file_name().unwrap().to_str().unwrap(),
            snapshot_package.parent().unwrap(),
            true,
        )
        .map_err(|err| format!("Failed to fetch snapshot: {:?}", err))?;
    }

    Ok(())
}

// This function is duplicated in ledger-tool/src/main.rs...
fn hardforks_of(matches: &ArgMatches<'_>, name: &str) -> Option<Vec<Slot>> {
    if matches.is_present(name) {
        Some(values_t_or_exit!(matches, name, Slot))
    } else {
        None
    }
}

#[allow(clippy::cognitive_complexity)]
pub fn main() {
    let default_dynamic_port_range =
        &format!("{}-{}", VALIDATOR_PORT_RANGE.0, VALIDATOR_PORT_RANGE.1);
    let default_limit_ledger_size = &DEFAULT_MAX_LEDGER_SLOTS.to_string();

    let matches = App::new(crate_name!()).about(crate_description!())
        .version(solana_clap_utils::version!())
        .arg(
            Arg::with_name("blockstream_unix_socket")
                .long("blockstream")
                .takes_value(true)
                .value_name("UNIX DOMAIN SOCKET")
                .help("Stream entries to this unix domain socket path")
        )
        .arg(
            Arg::with_name(ASK_SEED_PHRASE_ARG.name)
                .long(ASK_SEED_PHRASE_ARG.long)
                .value_name("KEYPAIR NAME")
                .multiple(true)
                .takes_value(true)
                .possible_values(&["identity-keypair", "storage-keypair", "voting-keypair"])
                .help(ASK_SEED_PHRASE_ARG.help),
        )
        .arg(
            Arg::with_name(SKIP_SEED_PHRASE_VALIDATION_ARG.name)
                .long(SKIP_SEED_PHRASE_VALIDATION_ARG.long)
                .requires(ASK_SEED_PHRASE_ARG.name)
                .help(SKIP_SEED_PHRASE_VALIDATION_ARG.help),
        )
        .arg(
            Arg::with_name("identity_keypair")
                .short("i")
                .long("identity-keypair")
                .value_name("PATH")
                .takes_value(true)
                .validator(is_keypair)
                .help("File containing the identity keypair for the validator"),
        )
        .arg(
            Arg::with_name("voting_keypair")
                .long("voting-keypair")
                .value_name("PATH")
                .takes_value(true)
                .validator(is_keypair)
                .help("File containing the authorized voting keypair.  Default is an ephemeral keypair, which may disable voting without --vote-account."),
        )
        .arg(
            Arg::with_name("vote_account")
                .long("vote-account")
                .value_name("PUBKEY")
                .takes_value(true)
                .validator(is_pubkey_or_keypair)
                .help("Public key of the vote account to vote with.  Default is the public key of --voting-keypair"),
        )
        .arg(
            Arg::with_name("storage_keypair")
                .long("storage-keypair")
                .value_name("PATH")
                .takes_value(true)
                .validator(is_keypair)
                .help("File containing the storage account keypair.  Default is an ephemeral keypair"),
        )
        .arg(
            Arg::with_name("init_complete_file")
                .long("init-complete-file")
                .value_name("FILE")
                .takes_value(true)
                .help("Create this file, if it doesn't already exist, once node initialization is complete"),
        )
        .arg(
            Arg::with_name("ledger_path")
                .short("l")
                .long("ledger")
                .value_name("DIR")
                .takes_value(true)
                .required(true)
                .help("Use DIR as persistent ledger location"),
        )
        .arg(
            Arg::with_name("entrypoint")
                .short("n")
                .long("entrypoint")
                .value_name("HOST:PORT")
                .takes_value(true)
                .validator(solana_net_utils::is_host_port)
                .help("Rendezvous with the cluster at this gossip entrypoint"),
        )
        .arg(
            Arg::with_name("no_snapshot_fetch")
                .long("no-snapshot-fetch")
                .takes_value(false)
                .requires("entrypoint")
                .help("Do not attempt to fetch a snapshot from the cluster, start from a local snapshot if present"),
        )
        .arg(
            Arg::with_name("no_genesis_fetch")
                .long("no-genesis-fetch")
                .takes_value(false)
                .requires("entrypoint")
                .help("Do not fetch genesis from the cluster"),
        )
        .arg(
            Arg::with_name("no_voting")
                .long("no-voting")
                .takes_value(false)
                .help("Launch node without voting"),
        )
        .arg(
            Arg::with_name("dev_no_sigverify")
                .long("dev-no-sigverify")
                .takes_value(false)
                .help("Run without signature verification"),
        )
        .arg(
            Arg::with_name("dev_halt_at_slot")
                .long("dev-halt-at-slot")
                .value_name("SLOT")
                .takes_value(true)
                .help("Halt the validator when it reaches the given slot"),
        )
        .arg(
            Arg::with_name("rpc_port")
                .long("rpc-port")
                .value_name("PORT")
                .takes_value(true)
                .validator(port_validator)
                .help("RPC port to use for this node"),
        )
        .arg(
            Arg::with_name("private_rpc")
                .long("--private-rpc")
                .takes_value(false)
                .help("Do not publish the RPC port for use by other nodes")
        )
        .arg(
            Arg::with_name("enable_rpc_exit")
                .long("enable-rpc-exit")
                .takes_value(false)
                .help("Enable the JSON RPC 'validatorExit' API.  Only enable in a debug environment"),
        )
        .arg(
            Arg::with_name("rpc_faucet_addr")
                .long("rpc-faucet-address")
                .value_name("HOST:PORT")
                .takes_value(true)
                .validator(solana_net_utils::is_host_port)
                .help("Enable the JSON RPC 'requestAirdrop' API with this faucet address."),
        )
        .arg(
            Arg::with_name("signer_addr")
                .long("vote-signer-address")
                .value_name("HOST:PORT")
                .takes_value(true)
                .hidden(true) // Don't document this argument to discourage its use
                .validator(solana_net_utils::is_host_port)
                .help("Rendezvous with the vote signer at this RPC end point"),
        )
        .arg(
            Arg::with_name("account_paths")
                .long("accounts")
                .value_name("PATHS")
                .takes_value(true)
                .help("Comma separated persistent accounts location"),
        )
        .arg(
            clap::Arg::with_name("gossip_port")
                .long("gossip-port")
                .value_name("PORT")
                .takes_value(true)
                .help("Gossip port number for the node"),
        )
        .arg(
            clap::Arg::with_name("gossip_host")
                .long("gossip-host")
                .value_name("HOST")
                .takes_value(true)
                .conflicts_with("entrypoint")
                .validator(solana_net_utils::is_host)
                .help("Gossip DNS name or IP address for the node when --entrypoint is not provided [default: 127.0.0.1]"),
        )
        .arg(
            clap::Arg::with_name("dynamic_port_range")
                .long("dynamic-port-range")
                .value_name("MIN_PORT-MAX_PORT")
                .takes_value(true)
                .default_value(default_dynamic_port_range)
                .validator(port_range_validator)
                .help("Range to use for dynamically assigned ports"),
        )
        .arg(
            clap::Arg::with_name("snapshot_interval_slots")
                .long("snapshot-interval-slots")
                .value_name("SNAPSHOT_INTERVAL_SLOTS")
                .takes_value(true)
                .default_value("100")
                .help("Number of slots between generating snapshots, 0 to disable snapshots"),
        )
        .arg(
            clap::Arg::with_name("limit_ledger_size")
                .long("limit-ledger-size")
                .value_name("SLOT_COUNT")
                .takes_value(true)
                .min_values(0)
                .max_values(1)
                .default_value(default_limit_ledger_size)
                .help("Drop ledger data for slots older than this value"),
        )
        .arg(
            clap::Arg::with_name("skip_poh_verify")
                .long("skip-poh-verify")
                .takes_value(false)
                .help("Skip ledger verification at node bootup"),
        )
        .arg(
            clap::Arg::with_name("cuda")
                .long("cuda")
                .takes_value(false)
                .help("Use CUDA"),
        )
        .arg(
            Arg::with_name("expected_genesis_hash")
                .long("expected-genesis-hash")
                .value_name("HASH")
                .takes_value(true)
                .validator(hash_validator)
                .help("Require the genesis have this hash"),
        )
        .arg(
            Arg::with_name("expected_shred_version")
                .long("expected-shred-version")
                .value_name("VERSION")
                .takes_value(true)
                .help("Require the shred version be this value"),
        )
        .arg(
            Arg::with_name("logfile")
                .short("o")
                .long("log")
                .value_name("FILE")
                .takes_value(true)
                .help("Redirect logging to the specified file, '-' for standard error"),
        )
        .arg(
            Arg::with_name("no_wait_for_supermajority")
                .long("no-wait-for-supermajority")
                .takes_value(false)
                .help("After processing the ledger, do not wait until a supermajority of stake is visible on gossip before starting PoH"),
        )
        .arg(
            // Legacy flag that is now enabled by default.  Remove this flag a couple months after the 0.23.0
            // release
            Arg::with_name("wait_for_supermajority")
                .long("wait-for-supermajority")
                .hidden(true)
        )
        .arg(
            Arg::with_name("hard_forks")
                .long("hard-fork")
                .value_name("SLOT")
                .multiple(true)
                .takes_value(true)
                .help("Add a hard fork at this slot"),
        )
        .get_matches();

    let identity_keypair = Arc::new(
        keypair_input(&matches, "identity-keypair")
            .unwrap_or_else(|err| {
                eprintln!("Identity keypair input failed: {}", err);
                exit(1);
            })
            .keypair,
    );
    let KeypairWithSource {
        keypair: voting_keypair,
        source: voting_keypair_source,
    } = keypair_input(&matches, "voting-keypair").unwrap_or_else(|err| {
        eprintln!("Voting keypair input failed: {}", err);
        exit(1);
    });
    let ephemeral_voting_keypair = voting_keypair_source == keypair::Source::Generated;
    let storage_keypair = keypair_input(&matches, "storage-keypair")
        .unwrap_or_else(|err| {
            eprintln!("Storage keypair input failed: {}", err);
            exit(1);
        })
        .keypair;

    let ledger_path = PathBuf::from(matches.value_of("ledger_path").unwrap());
    let entrypoint = matches.value_of("entrypoint");
    let init_complete_file = matches.value_of("init_complete_file");
    let skip_poh_verify = matches.is_present("skip_poh_verify");
    let cuda = matches.is_present("cuda");
    let no_genesis_fetch = matches.is_present("no_genesis_fetch");
    let no_snapshot_fetch = matches.is_present("no_snapshot_fetch");
    let private_rpc = matches.is_present("private_rpc");

    // Canonicalize ledger path to avoid issues with symlink creation
    let _ = fs::create_dir_all(&ledger_path);
    let ledger_path = fs::canonicalize(&ledger_path).unwrap_or_else(|err| {
        eprintln!("Unable to access ledger path: {:?}", err);
        exit(1);
    });

    let mut validator_config = ValidatorConfig {
        blockstream_unix_socket: matches
            .value_of("blockstream_unix_socket")
            .map(PathBuf::from),
        dev_sigverify_disabled: matches.is_present("dev_no_sigverify"),
        dev_halt_at_slot: value_t!(matches, "dev_halt_at_slot", Slot).ok(),
        expected_genesis_hash: matches
            .value_of("expected_genesis_hash")
            .map(|s| Hash::from_str(&s).unwrap()),
        expected_shred_version: value_t!(matches, "expected_shred_version", u16).ok(),
        new_hard_forks: hardforks_of(&matches, "hard_forks"),
        rpc_config: JsonRpcConfig {
            enable_validator_exit: matches.is_present("enable_rpc_exit"),
            faucet_addr: matches.value_of("rpc_faucet_addr").map(|address| {
                solana_net_utils::parse_host_port(address).expect("failed to parse faucet address")
            }),
        },
        rpc_ports: value_t!(matches, "rpc_port", u16)
            .ok()
            .map(|rpc_port| (rpc_port, rpc_port + 1)),
        voting_disabled: matches.is_present("no_voting"),
        wait_for_supermajority: !matches.is_present("no_wait_for_supermajority"),
        ..ValidatorConfig::default()
    };

    let dynamic_port_range =
        solana_net_utils::parse_port_range(matches.value_of("dynamic_port_range").unwrap())
            .expect("invalid dynamic_port_range");

    let account_paths = if let Some(account_paths) = matches.value_of("account_paths") {
        account_paths.split(',').map(PathBuf::from).collect()
    } else {
        vec![ledger_path.join("accounts")]
    };

    // Create and canonicalize account paths to avoid issues with symlink creation
    validator_config.account_paths = account_paths
        .into_iter()
        .map(|account_path| {
            match fs::create_dir_all(&account_path).and_then(|_| fs::canonicalize(&account_path)) {
                Ok(account_path) => account_path,
                Err(err) => {
                    eprintln!(
                        "Unable to access account path: {:?}, err: {:?}",
                        account_path, err
                    );
                    exit(1);
                }
            }
        })
        .collect();

    let snapshot_interval_slots = value_t_or_exit!(matches, "snapshot_interval_slots", usize);
    let snapshot_path = ledger_path.clone().join("snapshot");
    fs::create_dir_all(&snapshot_path).unwrap_or_else(|err| {
        eprintln!(
            "Failed to create snapshots directory {:?}: {}",
            snapshot_path, err
        );
        exit(1);
    });

    validator_config.snapshot_config = Some(SnapshotConfig {
        snapshot_interval_slots: if snapshot_interval_slots > 0 {
            snapshot_interval_slots
        } else {
            std::usize::MAX
        },
        snapshot_path,
        snapshot_package_output_path: ledger_path.clone(),
    });

    if matches.is_present("limit_ledger_size") {
        let limit_ledger_size = value_t_or_exit!(matches, "limit_ledger_size", u64);
        if limit_ledger_size < DEFAULT_MAX_LEDGER_SLOTS {
            eprintln!(
                "The provided --limit-ledger-size value was too small, the minimum value is {}",
                DEFAULT_MAX_LEDGER_SLOTS
            );
            exit(1);
        }
        validator_config.max_ledger_slots = Some(limit_ledger_size);
    }

    if matches.value_of("signer_addr").is_some() {
        warn!("--vote-signer-address ignored");
    }

    println!(
        "{} {}",
        style(crate_name!()).bold(),
        solana_clap_utils::version!()
    );

    let _log_redirect = {
        #[cfg(unix)]
        {
            let default_logfile = format!(
                "solana-validator-{}-{}.log",
                identity_keypair.pubkey(),
                chrono::Utc::now().format("%Y%m%d-%H%M%S")
            );
            let logfile = matches.value_of("logfile").unwrap_or(&default_logfile);

            if logfile == "-" {
                None
            } else {
                println!("log file: {}", logfile);
                Some(gag::Redirect::stderr(File::create(logfile).unwrap_or_else(
                    |err| {
                        eprintln!("Unable to create {}: {:?}", logfile, err);
                        exit(1);
                    },
                )))
            }
        }
        #[cfg(not(unix))]
        {
            println!("logging to a file is not supported on this platform");
            ()
        }
    };

    solana_logger::setup_with_default(
        &[
            "solana=info", /* info logging for all solana modules */
            "rpc=trace",   /* json_rpc request/response logging */
        ]
        .join(","),
    );

    info!("Starting validator with: {:#?}", std::env::args_os());

    let vote_account = pubkey_of(&matches, "vote_account").unwrap_or_else(|| {
        // Disable voting because normal (=not bootstrapping) validator rejects
        // non-voting accounts (= ephemeral keypairs).
        if ephemeral_voting_keypair && entrypoint.is_some() {
            warn!("Disabled voting due to the use of ephemeral key for vote account");
            validator_config.voting_disabled = true;
        };
        voting_keypair.pubkey()
    });

    solana_metrics::set_host_id(identity_keypair.pubkey().to_string());
    solana_metrics::set_panic_hook("validator");

    if cuda {
        solana_perf::perf_libs::init_cuda();
        enable_recycler_warming();
    }

    let entrypoint_addr = matches.value_of("entrypoint").map(|entrypoint| {
        solana_net_utils::parse_host_port(entrypoint).unwrap_or_else(|e| {
            eprintln!("failed to parse entrypoint address: {}", e);
            exit(1);
        })
    });

    let gossip_host = if let Some(entrypoint_addr) = entrypoint_addr {
        solana_net_utils::get_public_ip_addr(&entrypoint_addr).unwrap_or_else(|err| {
            eprintln!(
                "Failed to contact cluster entrypoint {}: {}",
                entrypoint_addr, err
            );
            exit(1);
        })
    } else {
        solana_net_utils::parse_host(matches.value_of("gossip_host").unwrap_or("127.0.0.1"))
            .unwrap_or_else(|err| {
                eprintln!("Error: {}", err);
                exit(1);
            })
    };

    let gossip_addr = SocketAddr::new(
        gossip_host,
        value_t!(matches, "gossip_port", u16).unwrap_or_else(|_| {
            solana_net_utils::find_available_port_in_range((0, 1))
                .expect("unable to find an available gossip port")
        }),
    );

    let cluster_entrypoint = entrypoint_addr
        .as_ref()
        .map(ContactInfo::new_gossip_entry_point);

    let mut tcp_ports = vec![];
    let mut node =
        Node::new_with_external_ip(&identity_keypair.pubkey(), &gossip_addr, dynamic_port_range);

    if !private_rpc {
        if let Some((rpc_port, rpc_pubsub_port)) = validator_config.rpc_ports {
            node.info.rpc = SocketAddr::new(node.info.gossip.ip(), rpc_port);
            node.info.rpc_pubsub = SocketAddr::new(node.info.gossip.ip(), rpc_pubsub_port);
            tcp_ports = vec![rpc_port, rpc_pubsub_port];
        }
    }

    if let Some(ref cluster_entrypoint) = cluster_entrypoint {
        let udp_sockets = vec![&node.sockets.gossip, &node.sockets.repair];

        let mut tcp_listeners: Vec<(_, _)> = tcp_ports
            .iter()
            .map(|port| {
                (
                    *port,
                    TcpListener::bind(&SocketAddr::from(([0, 0, 0, 0], *port))).unwrap_or_else(
                        |err| {
                            error!("Unable to bind to tcp/{}: {}", port, err);
                            std::process::exit(1);
                        },
                    ),
                )
            })
            .collect();
        if let Some(ip_echo) = &node.sockets.ip_echo {
            let ip_echo = ip_echo.try_clone().expect("unable to clone tcp_listener");
            tcp_listeners.push((node.info.gossip.port(), ip_echo));
        }

        solana_net_utils::verify_reachable_ports(
            &cluster_entrypoint.gossip,
            tcp_listeners,
            &udp_sockets,
        );

        if !no_genesis_fetch {
            let (rpc_client, rpc_addr) = get_rpc_addr(
                &node,
                &identity_keypair,
                &cluster_entrypoint.gossip,
                validator_config.expected_shred_version,
            );

            download_ledger(&rpc_addr, &ledger_path, no_snapshot_fetch).unwrap_or_else(|err| {
                error!("Failed to initialize ledger: {}", err);
                exit(1);
            });

            let genesis_hash = rpc_client.get_genesis_hash().unwrap_or_else(|err| {
                error!("Failed to get genesis hash: {}", err);
                exit(1);
            });

            if let Some(expected_genesis_hash) = validator_config.expected_genesis_hash {
                if expected_genesis_hash != genesis_hash {
                    error!(
                        "Genesis hash mismatch: expected {} but local genesis hash is {}",
                        expected_genesis_hash, genesis_hash,
                    );
                    exit(1);
                }
            }
            validator_config.expected_genesis_hash = Some(genesis_hash);

            if !validator_config.voting_disabled {
                check_vote_account(
                    &rpc_client,
                    &vote_account,
                    &voting_keypair.pubkey(),
                    &identity_keypair.pubkey(),
                )
                .unwrap_or_else(|err| {
                    error!("Failed to check vote account: {}", err);
                    exit(1);
                });
            }
        }
    }

    if !ledger_path.is_dir() {
        error!(
            "ledger directory does not exist or is not accessible: {:?}",
            ledger_path
        );
        exit(1);
    }

    let validator = Validator::new(
        node,
        &identity_keypair,
        &ledger_path,
        &vote_account,
        &Arc::new(voting_keypair),
        &Arc::new(storage_keypair),
        cluster_entrypoint.as_ref(),
        !skip_poh_verify,
        &validator_config,
    );

    if let Some(filename) = init_complete_file {
        File::create(filename).unwrap_or_else(|_| {
            error!("Unable to create: {}", filename);
            exit(1);
        });
    }
    info!("Validator initialized");
    validator.join().expect("validator exit");
    info!("Validator exiting..");
}
