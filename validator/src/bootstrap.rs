use {
    itertools::Itertools,
    log::*,
    rand::{seq::SliceRandom, thread_rng, Rng},
    rayon::prelude::*,
    solana_core::validator::{ValidatorConfig, ValidatorStartProgress},
    solana_download_utils::{download_snapshot_archive, DownloadProgressRecord},
    solana_genesis_utils::download_then_check_genesis_hash,
    solana_gossip::{
        cluster_info::{ClusterInfo, Node},
        contact_info::Protocol,
        crds_value,
        gossip_service::GossipService,
        legacy_contact_info::LegacyContactInfo as ContactInfo,
    },
    solana_metrics::datapoint_info,
    solana_rpc_client::rpc_client::RpcClient,
    solana_runtime::{
        snapshot_archive_info::SnapshotArchiveInfoGetter, snapshot_package::SnapshotKind,
        snapshot_utils,
    },
    solana_sdk::{
        clock::Slot,
        commitment_config::CommitmentConfig,
        hash::Hash,
        pubkey::Pubkey,
        signature::{Keypair, Signer},
    },
    solana_streamer::socket::SocketAddrSpace,
    std::{
        collections::{hash_map::RandomState, HashMap, HashSet},
        net::{SocketAddr, TcpListener, TcpStream, UdpSocket},
        path::Path,
        process::exit,
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc, RwLock,
        },
        time::{Duration, Instant},
    },
    thiserror::Error,
};

/// When downloading snapshots, wait at most this long for snapshot hashes from
/// _all_ known validators.  Afterwards, wait for snapshot hashes from _any_
/// known validator.
const WAIT_FOR_ALL_KNOWN_VALIDATORS: Duration = Duration::from_secs(60);
/// If we don't have any alternative peers after this long, better off trying
/// blacklisted peers again.
const BLACKLIST_CLEAR_THRESHOLD: Duration = Duration::from_secs(60);
/// If we can't find a good snapshot download candidate after this time, just
/// give up.
const NEWER_SNAPSHOT_THRESHOLD: Duration = Duration::from_secs(180);
/// If we haven't found any RPC peers after this time, just give up.
const GET_RPC_PEERS_TIMEOUT: Duration = Duration::from_secs(300);

pub const MAX_RPC_CONNECTIONS_EVALUATED_PER_ITERATION: usize = 32;

pub const PING_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Debug)]
pub struct RpcBootstrapConfig {
    pub no_genesis_fetch: bool,
    pub no_snapshot_fetch: bool,
    pub only_known_rpc: bool,
    pub max_genesis_archive_unpacked_size: u64,
    pub check_vote_account: Option<String>,
    pub incremental_snapshot_fetch: bool,
}

fn verify_reachable_ports(
    node: &Node,
    cluster_entrypoint: &ContactInfo,
    validator_config: &ValidatorConfig,
    socket_addr_space: &SocketAddrSpace,
) -> bool {
    let verify_address = |addr: &Option<SocketAddr>| -> bool {
        addr.as_ref()
            .map(|addr| socket_addr_space.check(addr))
            .unwrap_or_default()
    };
    let mut udp_sockets = vec![&node.sockets.gossip, &node.sockets.repair];

    if verify_address(&node.info.serve_repair(Protocol::UDP).ok()) {
        udp_sockets.push(&node.sockets.serve_repair);
    }
    if verify_address(&node.info.tpu(Protocol::UDP).ok()) {
        udp_sockets.extend(node.sockets.tpu.iter());
        udp_sockets.push(&node.sockets.tpu_quic);
    }
    if verify_address(&node.info.tpu_forwards(Protocol::UDP).ok()) {
        udp_sockets.extend(node.sockets.tpu_forwards.iter());
        udp_sockets.push(&node.sockets.tpu_forwards_quic);
    }
    if verify_address(&node.info.tpu_vote().ok()) {
        udp_sockets.extend(node.sockets.tpu_vote.iter());
    }
    if verify_address(&node.info.tvu(Protocol::UDP).ok()) {
        udp_sockets.extend(node.sockets.tvu.iter());
        udp_sockets.extend(node.sockets.broadcast.iter());
        udp_sockets.extend(node.sockets.retransmit_sockets.iter());
    }

    let mut tcp_listeners = vec![];
    if let Some((rpc_addr, rpc_pubsub_addr)) = validator_config.rpc_addrs {
        for (purpose, bind_addr, public_addr) in &[
            ("RPC", rpc_addr, node.info.rpc()),
            ("RPC pubsub", rpc_pubsub_addr, node.info.rpc_pubsub()),
        ] {
            if verify_address(&public_addr.as_ref().ok().copied()) {
                tcp_listeners.push((
                    bind_addr.port(),
                    TcpListener::bind(bind_addr).unwrap_or_else(|err| {
                        error!("Unable to bind to tcp {bind_addr:?} for {purpose}: {err}");
                        exit(1);
                    }),
                ));
            }
        }
    }

    if let Some(ip_echo) = &node.sockets.ip_echo {
        let ip_echo = ip_echo.try_clone().expect("unable to clone tcp_listener");
        tcp_listeners.push((ip_echo.local_addr().unwrap().port(), ip_echo));
    }

    solana_net_utils::verify_reachable_ports(
        &cluster_entrypoint.gossip().unwrap(),
        tcp_listeners,
        &udp_sockets,
    )
}

fn is_known_validator(id: &Pubkey, known_validators: &Option<HashSet<Pubkey>>) -> bool {
    if let Some(known_validators) = known_validators {
        known_validators.contains(id)
    } else {
        false
    }
}

fn start_gossip_node(
    identity_keypair: Arc<Keypair>,
    cluster_entrypoints: &[ContactInfo],
    ledger_path: &Path,
    gossip_addr: &SocketAddr,
    gossip_socket: UdpSocket,
    expected_shred_version: Option<u16>,
    gossip_validators: Option<HashSet<Pubkey>>,
    should_check_duplicate_instance: bool,
    socket_addr_space: SocketAddrSpace,
) -> (Arc<ClusterInfo>, Arc<AtomicBool>, GossipService) {
    let contact_info = ClusterInfo::gossip_contact_info(
        identity_keypair.pubkey(),
        *gossip_addr,
        expected_shred_version.unwrap_or(0),
    );
    let mut cluster_info = ClusterInfo::new(contact_info, identity_keypair, socket_addr_space);
    cluster_info.set_entrypoints(cluster_entrypoints.to_vec());
    cluster_info.restore_contact_info(ledger_path, 0);
    let cluster_info = Arc::new(cluster_info);

    let gossip_exit_flag = Arc::new(AtomicBool::new(false));
    let gossip_service = GossipService::new(
        &cluster_info,
        None,
        gossip_socket,
        gossip_validators,
        should_check_duplicate_instance,
        None,
        gossip_exit_flag.clone(),
    );
    (cluster_info, gossip_exit_flag, gossip_service)
}

fn get_rpc_peers(
    cluster_info: &ClusterInfo,
    cluster_entrypoints: &[ContactInfo],
    validator_config: &ValidatorConfig,
    blacklisted_rpc_nodes: &mut HashSet<Pubkey>,
    blacklist_timeout: &Instant,
    retry_reason: &mut Option<String>,
    bootstrap_config: &RpcBootstrapConfig,
) -> Vec<ContactInfo> {
    let shred_version = validator_config
        .expected_shred_version
        .unwrap_or_else(|| cluster_info.my_shred_version());
    if shred_version == 0 {
        let all_zero_shred_versions = cluster_entrypoints.iter().all(|cluster_entrypoint| {
            cluster_entrypoint
                .gossip()
                .ok()
                .and_then(|addr| cluster_info.lookup_contact_info_by_gossip_addr(&addr))
                .map_or(false, |entrypoint| entrypoint.shred_version() == 0)
        });

        if all_zero_shred_versions {
            eprintln!("Entrypoint shred version is zero.  Restart with --expected-shred-version");
            exit(1);
        }
        info!("Waiting to adopt entrypoint shred version...");
        return vec![];
    }

    info!(
        "Searching for an RPC service with shred version {shred_version}{}...",
        retry_reason
            .as_ref()
            .map(|s| format!(" (Retrying: {s})"))
            .unwrap_or_default()
    );

    let mut rpc_peers = cluster_info
        .all_rpc_peers()
        .into_iter()
        .filter(|contact_info| contact_info.shred_version() == shred_version)
        .collect::<Vec<_>>();

    if bootstrap_config.only_known_rpc {
        rpc_peers.retain(|rpc_peer| {
            is_known_validator(rpc_peer.pubkey(), &validator_config.known_validators)
        });
    }

    let rpc_peers_total = rpc_peers.len();

    // Filter out blacklisted nodes
    let rpc_peers: Vec<_> = rpc_peers
        .into_iter()
        .filter(|rpc_peer| !blacklisted_rpc_nodes.contains(rpc_peer.pubkey()))
        .collect();
    let rpc_peers_blacklisted = rpc_peers_total - rpc_peers.len();
    let rpc_known_peers = rpc_peers
        .iter()
        .filter(|rpc_peer| {
            is_known_validator(rpc_peer.pubkey(), &validator_config.known_validators)
        })
        .count();

    info!("Total {rpc_peers_total} RPC nodes found. {rpc_known_peers} known, {rpc_peers_blacklisted} blacklisted");

    if rpc_peers_blacklisted == rpc_peers_total {
        *retry_reason = if !blacklisted_rpc_nodes.is_empty()
            && blacklist_timeout.elapsed() > BLACKLIST_CLEAR_THRESHOLD
        {
            // All nodes are blacklisted and no additional nodes recently discovered.
            // Remove all nodes from the blacklist and try them again.
            blacklisted_rpc_nodes.clear();
            Some("Blacklist timeout expired".to_owned())
        } else {
            Some("Wait for known rpc peers".to_owned())
        };
        return vec![];
    }
    rpc_peers
}

fn check_vote_account(
    rpc_client: &RpcClient,
    identity_pubkey: &Pubkey,
    vote_account_address: &Pubkey,
    authorized_voter_pubkeys: &[Pubkey],
) -> Result<(), String> {
    let vote_account = rpc_client
        .get_account_with_commitment(vote_account_address, CommitmentConfig::confirmed())
        .map_err(|err| format!("failed to fetch vote account: {err}"))?
        .value
        .ok_or_else(|| format!("vote account does not exist: {vote_account_address}"))?;

    if vote_account.owner != solana_vote_program::id() {
        return Err(format!(
            "not a vote account (owned by {}): {}",
            vote_account.owner, vote_account_address
        ));
    }

    let identity_account = rpc_client
        .get_account_with_commitment(identity_pubkey, CommitmentConfig::confirmed())
        .map_err(|err| format!("failed to fetch identity account: {err}"))?
        .value
        .ok_or_else(|| format!("identity account does not exist: {identity_pubkey}"))?;

    let vote_state = solana_vote_program::vote_state::from(&vote_account);
    if let Some(vote_state) = vote_state {
        if vote_state.authorized_voters().is_empty() {
            return Err("Vote account not yet initialized".to_string());
        }

        if vote_state.node_pubkey != *identity_pubkey {
            return Err(format!(
                "vote account's identity ({}) does not match the validator's identity {}).",
                vote_state.node_pubkey, identity_pubkey
            ));
        }

        for (_, vote_account_authorized_voter_pubkey) in vote_state.authorized_voters().iter() {
            if !authorized_voter_pubkeys.contains(vote_account_authorized_voter_pubkey) {
                return Err(format!(
                    "authorized voter {vote_account_authorized_voter_pubkey} not available"
                ));
            }
        }
    } else {
        return Err(format!(
            "invalid vote account data for {vote_account_address}"
        ));
    }

    // Maybe we can calculate minimum voting fee; rather than 1 lamport
    if identity_account.lamports <= 1 {
        return Err(format!(
            "underfunded identity account ({}): only {} lamports available",
            identity_pubkey, identity_account.lamports
        ));
    }

    Ok(())
}

#[derive(Error, Debug)]
pub enum GetRpcNodeError {
    #[error("Unable to find any RPC peers")]
    NoRpcPeersFound,

    #[error("Giving up, did not get newer snapshots from the cluster")]
    NoNewerSnapshots,
}

/// Struct to wrap the return value from get_rpc_nodes().  The `rpc_contact_info` is the peer to
/// download from, and `snapshot_hash` is the (optional) full and (optional) incremental
/// snapshots to download.
#[derive(Debug)]
struct GetRpcNodeResult {
    rpc_contact_info: ContactInfo,
    snapshot_hash: Option<SnapshotHash>,
}

/// Struct to wrap the peers & snapshot hashes together.
#[derive(Debug, PartialEq, Eq, Clone)]
struct PeerSnapshotHash {
    rpc_contact_info: ContactInfo,
    snapshot_hash: SnapshotHash,
}

/// A snapshot hash.  In this context (bootstrap *with* incremental snapshots), a snapshot hash
/// is _both_ a full snapshot hash and an (optional) incremental snapshot hash.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Clone, Copy)]
pub struct SnapshotHash {
    full: (Slot, Hash),
    incr: Option<(Slot, Hash)>,
}

pub fn fail_rpc_node(
    err: String,
    known_validators: &Option<HashSet<Pubkey, RandomState>>,
    rpc_id: &Pubkey,
    blacklisted_rpc_nodes: &mut HashSet<Pubkey, RandomState>,
) {
    warn!("{err}");
    if let Some(ref known_validators) = known_validators {
        if known_validators.contains(rpc_id) {
            return;
        }
    }

    info!("Excluding {rpc_id} as a future RPC candidate");
    blacklisted_rpc_nodes.insert(*rpc_id);
}

fn shutdown_gossip_service(gossip: (Arc<ClusterInfo>, Arc<AtomicBool>, GossipService)) {
    let (cluster_info, gossip_exit_flag, gossip_service) = gossip;
    cluster_info.save_contact_info();
    gossip_exit_flag.store(true, Ordering::Relaxed);
    gossip_service.join().unwrap();
}

#[allow(clippy::too_many_arguments)]
pub fn attempt_download_genesis_and_snapshot(
    rpc_contact_info: &ContactInfo,
    ledger_path: &Path,
    validator_config: &mut ValidatorConfig,
    bootstrap_config: &RpcBootstrapConfig,
    use_progress_bar: bool,
    gossip: &mut Option<(Arc<ClusterInfo>, Arc<AtomicBool>, GossipService)>,
    rpc_client: &RpcClient,
    full_snapshot_archives_dir: &Path,
    incremental_snapshot_archives_dir: &Path,
    maximum_local_snapshot_age: Slot,
    start_progress: &Arc<RwLock<ValidatorStartProgress>>,
    minimal_snapshot_download_speed: f32,
    maximum_snapshot_download_abort: u64,
    download_abort_count: &mut u64,
    snapshot_hash: Option<SnapshotHash>,
    identity_keypair: &Arc<Keypair>,
    vote_account: &Pubkey,
    authorized_voter_keypairs: Arc<RwLock<Vec<Arc<Keypair>>>>,
) -> Result<(), String> {
    download_then_check_genesis_hash(
        &rpc_contact_info.rpc().map_err(|err| format!("{err:?}"))?,
        ledger_path,
        &mut validator_config.expected_genesis_hash,
        bootstrap_config.max_genesis_archive_unpacked_size,
        bootstrap_config.no_genesis_fetch,
        use_progress_bar,
        rpc_client,
    )?;

    if let Some(gossip) = gossip.take() {
        shutdown_gossip_service(gossip);
    }

    let rpc_client_slot = rpc_client
        .get_slot_with_commitment(CommitmentConfig::finalized())
        .map_err(|err| format!("Failed to get RPC node slot: {err}"))?;
    info!("RPC node root slot: {rpc_client_slot}");

    download_snapshots(
        full_snapshot_archives_dir,
        incremental_snapshot_archives_dir,
        validator_config,
        bootstrap_config,
        use_progress_bar,
        maximum_local_snapshot_age,
        start_progress,
        minimal_snapshot_download_speed,
        maximum_snapshot_download_abort,
        download_abort_count,
        snapshot_hash,
        rpc_contact_info,
    )?;

    if let Some(url) = bootstrap_config.check_vote_account.as_ref() {
        let rpc_client = RpcClient::new(url);
        check_vote_account(
            &rpc_client,
            &identity_keypair.pubkey(),
            vote_account,
            &authorized_voter_keypairs
                .read()
                .unwrap()
                .iter()
                .map(|k| k.pubkey())
                .collect::<Vec<_>>(),
        )
        .unwrap_or_else(|err| {
            // Consider failures here to be more likely due to user error (eg,
            // incorrect `solana-validator` command-line arguments) rather than the
            // RPC node failing.
            //
            // Power users can always use the `--no-check-vote-account` option to
            // bypass this check entirely
            error!("{err}");
            exit(1);
        });
    }
    Ok(())
}

/// simple ping helper function which returns the time to connect
fn ping(addr: &SocketAddr) -> Option<Duration> {
    let start = Instant::now();
    match TcpStream::connect_timeout(addr, PING_TIMEOUT) {
        Ok(_) => Some(start.elapsed()),
        Err(_) => None,
    }
}

// Populates `vetted_rpc_nodes` with a list of RPC nodes that are ready to be
// used for downloading latest snapshots and/or the genesis block. Guaranteed to
// find at least one viable node or terminate the process.
fn get_vetted_rpc_nodes(
    vetted_rpc_nodes: &mut Vec<(ContactInfo, Option<SnapshotHash>, RpcClient)>,
    cluster_info: &Arc<ClusterInfo>,
    cluster_entrypoints: &[ContactInfo],
    validator_config: &ValidatorConfig,
    blacklisted_rpc_nodes: &mut HashSet<Pubkey>,
    bootstrap_config: &RpcBootstrapConfig,
) {
    while vetted_rpc_nodes.is_empty() {
        let rpc_node_details = match get_rpc_nodes(
            cluster_info,
            cluster_entrypoints,
            validator_config,
            blacklisted_rpc_nodes,
            bootstrap_config,
        ) {
            Ok(rpc_node_details) => rpc_node_details,
            Err(err) => {
                error!(
                    "Failed to get RPC nodes: {err}. Consider checking system \
                    clock, removing `--no-port-check`, or adjusting \
                    `--known-validator ...` arguments as applicable"
                );
                exit(1);
            }
        };

        let newly_blacklisted_rpc_nodes = RwLock::new(HashSet::new());
        vetted_rpc_nodes.extend(
            rpc_node_details
                .into_par_iter()
                .filter_map(|rpc_node_details| {
                    let GetRpcNodeResult {
                        rpc_contact_info,
                        snapshot_hash,
                    } = rpc_node_details;

                    info!(
                        "Using RPC service from node {}: {:?}",
                        rpc_contact_info.pubkey(),
                        rpc_contact_info.rpc()
                    );

                    let rpc_addr = rpc_contact_info.rpc().ok()?;
                    let ping_time = ping(&rpc_addr);

                    let rpc_client =
                        RpcClient::new_socket_with_timeout(rpc_addr, Duration::from_secs(5));

                    Some((rpc_contact_info, snapshot_hash, rpc_client, ping_time))
                })
                .filter(
                    |(rpc_contact_info, _snapshot_hash, rpc_client, ping_time)| match rpc_client
                        .get_version()
                    {
                        Ok(rpc_version) => {
                            if let Some(ping_time) = ping_time {
                                info!(
                                    "RPC node version: {} Ping: {}ms",
                                    rpc_version.solana_core,
                                    ping_time.as_millis()
                                );
                                true
                            } else {
                                fail_rpc_node(
                                    "Failed to ping RPC".to_string(),
                                    &validator_config.known_validators,
                                    rpc_contact_info.pubkey(),
                                    &mut newly_blacklisted_rpc_nodes.write().unwrap(),
                                );
                                false
                            }
                        }
                        Err(err) => {
                            fail_rpc_node(
                                format!("Failed to get RPC node version: {err}"),
                                &validator_config.known_validators,
                                rpc_contact_info.pubkey(),
                                &mut newly_blacklisted_rpc_nodes.write().unwrap(),
                            );
                            false
                        }
                    },
                )
                .collect::<Vec<(
                    ContactInfo,
                    Option<SnapshotHash>,
                    RpcClient,
                    Option<Duration>,
                )>>()
                .into_iter()
                .sorted_by_key(|(_, _, _, ping_time)| ping_time.unwrap())
                .map(|(rpc_contact_info, snapshot_hash, rpc_client, _)| {
                    (rpc_contact_info, snapshot_hash, rpc_client)
                })
                .collect::<Vec<(ContactInfo, Option<SnapshotHash>, RpcClient)>>(),
        );
        blacklisted_rpc_nodes.extend(newly_blacklisted_rpc_nodes.into_inner().unwrap());
    }
}

#[allow(clippy::too_many_arguments)]
pub fn rpc_bootstrap(
    node: &Node,
    identity_keypair: &Arc<Keypair>,
    ledger_path: &Path,
    full_snapshot_archives_dir: &Path,
    incremental_snapshot_archives_dir: &Path,
    vote_account: &Pubkey,
    authorized_voter_keypairs: Arc<RwLock<Vec<Arc<Keypair>>>>,
    cluster_entrypoints: &[ContactInfo],
    validator_config: &mut ValidatorConfig,
    bootstrap_config: RpcBootstrapConfig,
    do_port_check: bool,
    use_progress_bar: bool,
    maximum_local_snapshot_age: Slot,
    should_check_duplicate_instance: bool,
    start_progress: &Arc<RwLock<ValidatorStartProgress>>,
    minimal_snapshot_download_speed: f32,
    maximum_snapshot_download_abort: u64,
    socket_addr_space: SocketAddrSpace,
) {
    if do_port_check {
        let mut order: Vec<_> = (0..cluster_entrypoints.len()).collect();
        order.shuffle(&mut thread_rng());
        if order.into_iter().all(|i| {
            !verify_reachable_ports(
                node,
                &cluster_entrypoints[i],
                validator_config,
                &socket_addr_space,
            )
        }) {
            exit(1);
        }
    }

    if bootstrap_config.no_genesis_fetch && bootstrap_config.no_snapshot_fetch {
        return;
    }

    let total_snapshot_download_time = Instant::now();
    let mut get_rpc_nodes_time = Duration::new(0, 0);
    let mut snapshot_download_time = Duration::new(0, 0);
    let mut blacklisted_rpc_nodes = HashSet::new();
    let mut gossip = None;
    let mut vetted_rpc_nodes = vec![];
    let mut download_abort_count = 0;
    loop {
        if gossip.is_none() {
            *start_progress.write().unwrap() = ValidatorStartProgress::SearchingForRpcService;

            gossip = Some(start_gossip_node(
                identity_keypair.clone(),
                cluster_entrypoints,
                ledger_path,
                &node
                    .info
                    .gossip()
                    .expect("Operator must spin up node with valid gossip address"),
                node.sockets.gossip.try_clone().unwrap(),
                validator_config.expected_shred_version,
                validator_config.gossip_validators.clone(),
                should_check_duplicate_instance,
                socket_addr_space,
            ));
        }

        let get_rpc_nodes_start = Instant::now();
        get_vetted_rpc_nodes(
            &mut vetted_rpc_nodes,
            &gossip.as_ref().unwrap().0,
            cluster_entrypoints,
            validator_config,
            &mut blacklisted_rpc_nodes,
            &bootstrap_config,
        );
        let (rpc_contact_info, snapshot_hash, rpc_client) = vetted_rpc_nodes.pop().unwrap();
        get_rpc_nodes_time += get_rpc_nodes_start.elapsed();

        let snapshot_download_start = Instant::now();
        let download_result = attempt_download_genesis_and_snapshot(
            &rpc_contact_info,
            ledger_path,
            validator_config,
            &bootstrap_config,
            use_progress_bar,
            &mut gossip,
            &rpc_client,
            full_snapshot_archives_dir,
            incremental_snapshot_archives_dir,
            maximum_local_snapshot_age,
            start_progress,
            minimal_snapshot_download_speed,
            maximum_snapshot_download_abort,
            &mut download_abort_count,
            snapshot_hash,
            identity_keypair,
            vote_account,
            authorized_voter_keypairs.clone(),
        );
        snapshot_download_time += snapshot_download_start.elapsed();
        match download_result {
            Ok(()) => break,
            Err(err) => {
                fail_rpc_node(
                    err,
                    &validator_config.known_validators,
                    rpc_contact_info.pubkey(),
                    &mut blacklisted_rpc_nodes,
                );
            }
        }
    }

    if let Some(gossip) = gossip.take() {
        shutdown_gossip_service(gossip);
    }

    datapoint_info!(
        "bootstrap-snapshot-download",
        (
            "total_time_secs",
            total_snapshot_download_time.elapsed().as_secs(),
            i64
        ),
        ("get_rpc_nodes_time_secs", get_rpc_nodes_time.as_secs(), i64),
        (
            "snapshot_download_time_secs",
            snapshot_download_time.as_secs(),
            i64
        ),
        ("download_abort_count", download_abort_count, i64),
        ("blacklisted_nodes_count", blacklisted_rpc_nodes.len(), i64),
    );
}

/// Get RPC peer node candidates to download from.
///
/// This function finds the highest compatible snapshots from the cluster and returns RPC peers.
fn get_rpc_nodes(
    cluster_info: &ClusterInfo,
    cluster_entrypoints: &[ContactInfo],
    validator_config: &ValidatorConfig,
    blacklisted_rpc_nodes: &mut HashSet<Pubkey>,
    bootstrap_config: &RpcBootstrapConfig,
) -> Result<Vec<GetRpcNodeResult>, GetRpcNodeError> {
    let mut blacklist_timeout = Instant::now();
    let mut get_rpc_peers_timout = Instant::now();
    let mut newer_cluster_snapshot_timeout = None;
    let mut retry_reason = None;
    loop {
        // Give gossip some time to populate and not spin on grabbing the crds lock
        std::thread::sleep(Duration::from_secs(1));
        info!("\n{}", cluster_info.rpc_info_trace());

        let rpc_peers = get_rpc_peers(
            cluster_info,
            cluster_entrypoints,
            validator_config,
            blacklisted_rpc_nodes,
            &blacklist_timeout,
            &mut retry_reason,
            bootstrap_config,
        );
        if rpc_peers.is_empty() {
            if get_rpc_peers_timout.elapsed() > GET_RPC_PEERS_TIMEOUT {
                return Err(GetRpcNodeError::NoRpcPeersFound);
            }
            continue;
        }

        // Reset timeouts if we found any viable RPC peers.
        blacklist_timeout = Instant::now();
        get_rpc_peers_timout = Instant::now();
        if bootstrap_config.no_snapshot_fetch {
            let random_peer = &rpc_peers[thread_rng().gen_range(0..rpc_peers.len())];
            return Ok(vec![GetRpcNodeResult {
                rpc_contact_info: random_peer.clone(),
                snapshot_hash: None,
            }]);
        }

        let known_validators_to_wait_for = if newer_cluster_snapshot_timeout
            .as_ref()
            .map(|timer: &Instant| timer.elapsed() < WAIT_FOR_ALL_KNOWN_VALIDATORS)
            .unwrap_or(true)
        {
            KnownValidatorsToWaitFor::All
        } else {
            KnownValidatorsToWaitFor::Any
        };
        let peer_snapshot_hashes = get_peer_snapshot_hashes(
            cluster_info,
            &rpc_peers,
            validator_config.known_validators.as_ref(),
            known_validators_to_wait_for,
            bootstrap_config.incremental_snapshot_fetch,
        );
        if peer_snapshot_hashes.is_empty() {
            match newer_cluster_snapshot_timeout {
                None => newer_cluster_snapshot_timeout = Some(Instant::now()),
                Some(newer_cluster_snapshot_timeout) => {
                    if newer_cluster_snapshot_timeout.elapsed() > NEWER_SNAPSHOT_THRESHOLD {
                        return Err(GetRpcNodeError::NoNewerSnapshots);
                    }
                }
            }
            retry_reason = Some("No snapshots available".to_owned());
            continue;
        } else {
            let rpc_peers = peer_snapshot_hashes
                .iter()
                .map(|peer_snapshot_hash| peer_snapshot_hash.rpc_contact_info.pubkey())
                .collect::<Vec<_>>();
            let final_snapshot_hash = peer_snapshot_hashes[0].snapshot_hash;
            info!(
                "Highest available snapshot slot is {}, available from {} node{}: {:?}",
                final_snapshot_hash
                    .incr
                    .map(|(slot, _hash)| slot)
                    .unwrap_or(final_snapshot_hash.full.0),
                rpc_peers.len(),
                if rpc_peers.len() > 1 { "s" } else { "" },
                rpc_peers,
            );
            let rpc_node_results = peer_snapshot_hashes
                .iter()
                .map(|peer_snapshot_hash| GetRpcNodeResult {
                    rpc_contact_info: peer_snapshot_hash.rpc_contact_info.clone(),
                    snapshot_hash: Some(peer_snapshot_hash.snapshot_hash),
                })
                .take(MAX_RPC_CONNECTIONS_EVALUATED_PER_ITERATION)
                .collect();
            return Ok(rpc_node_results);
        }
    }
}

/// Get the Slot and Hash of the local snapshot with the highest slot.  Can be either a full
/// snapshot or an incremental snapshot.
fn get_highest_local_snapshot_hash(
    full_snapshot_archives_dir: impl AsRef<Path>,
    incremental_snapshot_archives_dir: impl AsRef<Path>,
    incremental_snapshot_fetch: bool,
) -> Option<(Slot, Hash)> {
    snapshot_utils::get_highest_full_snapshot_archive_info(full_snapshot_archives_dir)
        .and_then(|full_snapshot_info| {
            if incremental_snapshot_fetch {
                snapshot_utils::get_highest_incremental_snapshot_archive_info(
                    incremental_snapshot_archives_dir,
                    full_snapshot_info.slot(),
                )
                .map(|incremental_snapshot_info| {
                    (
                        incremental_snapshot_info.slot(),
                        *incremental_snapshot_info.hash(),
                    )
                })
            } else {
                None
            }
            .or_else(|| Some((full_snapshot_info.slot(), *full_snapshot_info.hash())))
        })
        .map(|(slot, snapshot_hash)| (slot, snapshot_hash.0))
}

/// Get peer snapshot hashes
///
/// The result is a vector of peers with snapshot hashes that:
/// 1. match a snapshot hash from the known validators
/// 2. have the highest incremental snapshot slot
/// 3. have the highest full snapshot slot of (2)
fn get_peer_snapshot_hashes(
    cluster_info: &ClusterInfo,
    rpc_peers: &[ContactInfo],
    known_validators: Option<&HashSet<Pubkey>>,
    known_validators_to_wait_for: KnownValidatorsToWaitFor,
    incremental_snapshot_fetch: bool,
) -> Vec<PeerSnapshotHash> {
    let mut peer_snapshot_hashes = get_eligible_peer_snapshot_hashes(cluster_info, rpc_peers);
    if let Some(known_validators) = known_validators {
        let known_snapshot_hashes = get_snapshot_hashes_from_known_validators(
            cluster_info,
            known_validators,
            known_validators_to_wait_for,
        );
        retain_peer_snapshot_hashes_that_match_known_snapshot_hashes(
            &known_snapshot_hashes,
            &mut peer_snapshot_hashes,
        );
    }
    if incremental_snapshot_fetch {
        // Only filter by highest incremental snapshot slot if we're actually going to download an
        // incremental snapshot.  Otherwise this could remove higher full snapshot slots from
        // being selected.  For example, if there are two peer snapshot hashes:
        // (A) full snapshot slot: 100, incremental snapshot slot: 160
        // (B) full snapshot slot: 150, incremental snapshot slot: None
        // Then (A) has the highest overall snapshot slot.  But if we're not downlading and
        // incremental snapshot, (B) should be selected since it's full snapshot of 150 is highest.
        retain_peer_snapshot_hashes_with_highest_incremental_snapshot_slot(
            &mut peer_snapshot_hashes,
        );
    }
    retain_peer_snapshot_hashes_with_highest_full_snapshot_slot(&mut peer_snapshot_hashes);

    peer_snapshot_hashes
}

/// Map full snapshot hashes to a set of incremental snapshot hashes.  Each full snapshot hash
/// is treated as the base for its set of incremental snapshot hashes.
type KnownSnapshotHashes = HashMap<(Slot, Hash), HashSet<(Slot, Hash)>>;

/// Get the snapshot hashes from known validators.
///
/// The snapshot hashes are put into a map from full snapshot hash to a set of incremental
/// snapshot hashes.  This map will be used as the "known snapshot hashes"; when peers are
/// queried for their individual snapshot hashes, their results will be checked against this
/// map to verify correctness.
///
/// NOTE: Only a single snashot hash is allowed per slot.  If somehow two known validators have
/// a snapshot hash with the same slot and _different_ hashes, the second will be skipped.
/// This applies to both full and incremental snapshot hashes.
fn get_snapshot_hashes_from_known_validators(
    cluster_info: &ClusterInfo,
    known_validators: &HashSet<Pubkey>,
    known_validators_to_wait_for: KnownValidatorsToWaitFor,
) -> KnownSnapshotHashes {
    // Get the snapshot hashes for a node from CRDS
    let get_snapshot_hashes_for_node = |node| get_snapshot_hashes_for_node(cluster_info, node);

    if !do_known_validators_have_all_snapshot_hashes(
        known_validators,
        known_validators_to_wait_for,
        get_snapshot_hashes_for_node,
    ) {
        debug!(
            "Snapshot hashes have not been discovered from known validators. \
            This likely means the gossip tables are not fully populated. \
            We will sleep and retry..."
        );
        return KnownSnapshotHashes::default();
    }

    build_known_snapshot_hashes(known_validators, get_snapshot_hashes_for_node)
}

/// Check if we can discover snapshot hashes for the known validators.
///
/// This is a heuristic to ensure the gossip tables are populated enough so that the bootstrap
/// process will download snapshots.
///
/// This function will return false if we do not yet have snapshot hashes from known validators;
/// and true otherwise.  Either require snapshot hashes from *all* or *any* of the known validators
/// based on the `KnownValidatorsToWaitFor` parameter.
fn do_known_validators_have_all_snapshot_hashes<'a>(
    known_validators: impl IntoIterator<Item = &'a Pubkey>,
    known_validators_to_wait_for: KnownValidatorsToWaitFor,
    get_snapshot_hashes_for_node: impl Fn(&'a Pubkey) -> Option<SnapshotHash>,
) -> bool {
    let node_has_snapshot_hashes = |node| get_snapshot_hashes_for_node(node).is_some();

    match known_validators_to_wait_for {
        KnownValidatorsToWaitFor::All => known_validators.into_iter().all(node_has_snapshot_hashes),
        KnownValidatorsToWaitFor::Any => known_validators.into_iter().any(node_has_snapshot_hashes),
    }
}

/// When waiting for snapshot hashes from the known validators, should we wait for *all* or *any*
/// of them?
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
enum KnownValidatorsToWaitFor {
    All,
    Any,
}

/// Build the known snapshot hashes from a set of nodes.
///
/// The `get_snapshot_hashes_for_node` parameter is a function that map a pubkey to its snapshot
/// hashes.  This parameter exist to provide a way to test the inner algorithm without needing
/// runtime information such as the ClusterInfo or ValidatorConfig.
fn build_known_snapshot_hashes<'a>(
    nodes: impl IntoIterator<Item = &'a Pubkey>,
    get_snapshot_hashes_for_node: impl Fn(&'a Pubkey) -> Option<SnapshotHash>,
) -> KnownSnapshotHashes {
    let mut known_snapshot_hashes = KnownSnapshotHashes::new();

    /// Check to see if there exists another snapshot hash in the haystack with the *same* slot
    /// but *different* hash as the needle.
    fn is_any_same_slot_and_different_hash<'a>(
        needle: &(Slot, Hash),
        haystack: impl IntoIterator<Item = &'a (Slot, Hash)>,
    ) -> bool {
        haystack
            .into_iter()
            .any(|hay| needle.0 == hay.0 && needle.1 != hay.1)
    }

    'to_next_node: for node in nodes {
        let Some(SnapshotHash {
            full: full_snapshot_hash,
            incr: incremental_snapshot_hash,
        }) = get_snapshot_hashes_for_node(node)
        else {
            continue 'to_next_node;
        };

        // Do not add this snapshot hash if there's already a full snapshot hash with the
        // same slot but with a _different_ hash.
        // NOTE: Nodes should not produce snapshots at the same slot with _different_
        // hashes.  So if it happens, keep the first and ignore the rest.
        if is_any_same_slot_and_different_hash(&full_snapshot_hash, known_snapshot_hashes.keys()) {
            warn!(
                "Ignoring all snapshot hashes from node {node} since we've seen a different full snapshot hash with this slot.\
                \nfull snapshot hash: {full_snapshot_hash:?}"
            );
            debug!(
                "known full snapshot hashes: {:#?}",
                known_snapshot_hashes.keys(),
            );
            continue 'to_next_node;
        }

        // Insert a new full snapshot hash into the known snapshot hashes IFF an entry
        // doesn't already exist.  This is to ensure we don't overwrite existing
        // incremental snapshot hashes that may be present for this full snapshot hash.
        let known_incremental_snapshot_hashes =
            known_snapshot_hashes.entry(full_snapshot_hash).or_default();

        if let Some(incremental_snapshot_hash) = incremental_snapshot_hash {
            // Do not add this snapshot hash if there's already an incremental snapshot
            // hash with the same slot, but with a _different_ hash.
            // NOTE: Nodes should not produce snapshots at the same slot with _different_
            // hashes.  So if it happens, keep the first and ignore the rest.
            if is_any_same_slot_and_different_hash(
                &incremental_snapshot_hash,
                known_incremental_snapshot_hashes.iter(),
            ) {
                warn!(
                    "Ignoring incremental snapshot hash from node {node} since we've seen a different incremental snapshot hash with this slot.\
                    \nfull snapshot hash: {full_snapshot_hash:?}\
                    \nincremental snapshot hash: {incremental_snapshot_hash:?}"
                );
                debug!(
                    "known incremental snapshot hashes based on this slot: {:#?}",
                    known_incremental_snapshot_hashes.iter(),
                );
                continue 'to_next_node;
            }

            known_incremental_snapshot_hashes.insert(incremental_snapshot_hash);
        };
    }

    trace!("known snapshot hashes: {known_snapshot_hashes:?}");
    known_snapshot_hashes
}

/// Get snapshot hashes from all eligible peers.
///
/// This fn will get only one snapshot hash per peer (the one with the highest slot).
/// This may be just a full snapshot hash, or a combo full snapshot hash and
/// incremental snapshot hash.
fn get_eligible_peer_snapshot_hashes(
    cluster_info: &ClusterInfo,
    rpc_peers: &[ContactInfo],
) -> Vec<PeerSnapshotHash> {
    let peer_snapshot_hashes = rpc_peers
        .iter()
        .flat_map(|rpc_peer| {
            get_snapshot_hashes_for_node(cluster_info, rpc_peer.pubkey()).map(|snapshot_hash| {
                PeerSnapshotHash {
                    rpc_contact_info: rpc_peer.clone(),
                    snapshot_hash,
                }
            })
        })
        .collect();

    trace!("peer snapshot hashes: {peer_snapshot_hashes:?}");
    peer_snapshot_hashes
}

/// Retain the peer snapshot hashes that match a snapshot hash from the known snapshot hashes
fn retain_peer_snapshot_hashes_that_match_known_snapshot_hashes(
    known_snapshot_hashes: &KnownSnapshotHashes,
    peer_snapshot_hashes: &mut Vec<PeerSnapshotHash>,
) {
    peer_snapshot_hashes.retain(|peer_snapshot_hash| {
        known_snapshot_hashes
            .get(&peer_snapshot_hash.snapshot_hash.full)
            .map(|known_incremental_hashes| {
                if peer_snapshot_hash.snapshot_hash.incr.is_none() {
                    // If the peer's full snapshot hashes match, but doesn't have any
                    // incremental snapshots, that's fine; keep 'em!
                    true
                } else {
                    known_incremental_hashes
                        .contains(peer_snapshot_hash.snapshot_hash.incr.as_ref().unwrap())
                }
            })
            .unwrap_or(false)
    });

    trace!(
        "retain peer snapshot hashes that match known snapshot hashes: {peer_snapshot_hashes:?}"
    );
}

/// Retain the peer snapshot hashes with the highest full snapshot slot
fn retain_peer_snapshot_hashes_with_highest_full_snapshot_slot(
    peer_snapshot_hashes: &mut Vec<PeerSnapshotHash>,
) {
    let highest_full_snapshot_hash = peer_snapshot_hashes
        .iter()
        .map(|peer_snapshot_hash| peer_snapshot_hash.snapshot_hash.full)
        .max_by_key(|(slot, _hash)| *slot);
    let Some(highest_full_snapshot_hash) = highest_full_snapshot_hash else {
        // `max_by_key` will only be `None` IFF the input `peer_snapshot_hashes` is empty.
        // In that case there's nothing to do (additionally, without a valid 'max' value, there
        // will be nothing to compare against within the `retain()` predicate).
        return;
    };

    peer_snapshot_hashes.retain(|peer_snapshot_hash| {
        peer_snapshot_hash.snapshot_hash.full == highest_full_snapshot_hash
    });

    trace!("retain peer snapshot hashes with highest full snapshot slot: {peer_snapshot_hashes:?}");
}

/// Retain the peer snapshot hashes with the highest incremental snapshot slot
fn retain_peer_snapshot_hashes_with_highest_incremental_snapshot_slot(
    peer_snapshot_hashes: &mut Vec<PeerSnapshotHash>,
) {
    let highest_incremental_snapshot_hash = peer_snapshot_hashes
        .iter()
        .flat_map(|peer_snapshot_hash| peer_snapshot_hash.snapshot_hash.incr)
        .max_by_key(|(slot, _hash)| *slot);

    peer_snapshot_hashes.retain(|peer_snapshot_hash| {
        peer_snapshot_hash.snapshot_hash.incr == highest_incremental_snapshot_hash
    });

    trace!("retain peer snapshot hashes with highest incremental snapshot slot: {peer_snapshot_hashes:?}");
}

/// Check to see if we can use our local snapshots, otherwise download newer ones.
#[allow(clippy::too_many_arguments)]
fn download_snapshots(
    full_snapshot_archives_dir: &Path,
    incremental_snapshot_archives_dir: &Path,
    validator_config: &ValidatorConfig,
    bootstrap_config: &RpcBootstrapConfig,
    use_progress_bar: bool,
    maximum_local_snapshot_age: Slot,
    start_progress: &Arc<RwLock<ValidatorStartProgress>>,
    minimal_snapshot_download_speed: f32,
    maximum_snapshot_download_abort: u64,
    download_abort_count: &mut u64,
    snapshot_hash: Option<SnapshotHash>,
    rpc_contact_info: &ContactInfo,
) -> Result<(), String> {
    if snapshot_hash.is_none() {
        return Ok(());
    }
    let SnapshotHash {
        full: full_snapshot_hash,
        incr: incremental_snapshot_hash,
    } = snapshot_hash.unwrap();

    // If the local snapshots are new enough, then use 'em; no need to download new snapshots
    if should_use_local_snapshot(
        full_snapshot_archives_dir,
        incremental_snapshot_archives_dir,
        maximum_local_snapshot_age,
        full_snapshot_hash,
        incremental_snapshot_hash,
        bootstrap_config.incremental_snapshot_fetch,
    ) {
        return Ok(());
    }

    // Check and see if we've already got the full snapshot; if not, download it
    if snapshot_utils::get_full_snapshot_archives(full_snapshot_archives_dir)
        .into_iter()
        .any(|snapshot_archive| {
            snapshot_archive.slot() == full_snapshot_hash.0
                && snapshot_archive.hash().0 == full_snapshot_hash.1
        })
    {
        info!(
            "Full snapshot archive already exists locally. Skipping download. slot: {}, hash: {}",
            full_snapshot_hash.0, full_snapshot_hash.1
        );
    } else {
        download_snapshot(
            full_snapshot_archives_dir,
            incremental_snapshot_archives_dir,
            validator_config,
            bootstrap_config,
            use_progress_bar,
            start_progress,
            minimal_snapshot_download_speed,
            maximum_snapshot_download_abort,
            download_abort_count,
            rpc_contact_info,
            full_snapshot_hash,
            SnapshotKind::FullSnapshot,
        )?;
    }

    if bootstrap_config.incremental_snapshot_fetch {
        // Check and see if we've already got the incremental snapshot; if not, download it
        if let Some(incremental_snapshot_hash) = incremental_snapshot_hash {
            if snapshot_utils::get_incremental_snapshot_archives(incremental_snapshot_archives_dir)
                .into_iter()
                .any(|snapshot_archive| {
                    snapshot_archive.slot() == incremental_snapshot_hash.0
                        && snapshot_archive.hash().0 == incremental_snapshot_hash.1
                        && snapshot_archive.base_slot() == full_snapshot_hash.0
                })
            {
                info!(
                    "Incremental snapshot archive already exists locally. Skipping download. slot: {}, hash: {}",
                    incremental_snapshot_hash.0, incremental_snapshot_hash.1
                );
            } else {
                download_snapshot(
                    full_snapshot_archives_dir,
                    incremental_snapshot_archives_dir,
                    validator_config,
                    bootstrap_config,
                    use_progress_bar,
                    start_progress,
                    minimal_snapshot_download_speed,
                    maximum_snapshot_download_abort,
                    download_abort_count,
                    rpc_contact_info,
                    incremental_snapshot_hash,
                    SnapshotKind::IncrementalSnapshot(full_snapshot_hash.0),
                )?;
            }
        }
    }

    Ok(())
}

/// Download a snapshot
#[allow(clippy::too_many_arguments)]
fn download_snapshot(
    full_snapshot_archives_dir: &Path,
    incremental_snapshot_archives_dir: &Path,
    validator_config: &ValidatorConfig,
    bootstrap_config: &RpcBootstrapConfig,
    use_progress_bar: bool,
    start_progress: &Arc<RwLock<ValidatorStartProgress>>,
    minimal_snapshot_download_speed: f32,
    maximum_snapshot_download_abort: u64,
    download_abort_count: &mut u64,
    rpc_contact_info: &ContactInfo,
    desired_snapshot_hash: (Slot, Hash),
    snapshot_kind: SnapshotKind,
) -> Result<(), String> {
    let maximum_full_snapshot_archives_to_retain = validator_config
        .snapshot_config
        .maximum_full_snapshot_archives_to_retain;
    let maximum_incremental_snapshot_archives_to_retain = validator_config
        .snapshot_config
        .maximum_incremental_snapshot_archives_to_retain;

    *start_progress.write().unwrap() = ValidatorStartProgress::DownloadingSnapshot {
        slot: desired_snapshot_hash.0,
        rpc_addr: rpc_contact_info.rpc().map_err(|err| format!("{err:?}"))?,
    };
    let desired_snapshot_hash = (
        desired_snapshot_hash.0,
        solana_runtime::snapshot_hash::SnapshotHash(desired_snapshot_hash.1),
    );
    download_snapshot_archive(
        &rpc_contact_info.rpc().map_err(|err| format!("{err:?}"))?,
        full_snapshot_archives_dir,
        incremental_snapshot_archives_dir,
        desired_snapshot_hash,
        snapshot_kind,
        maximum_full_snapshot_archives_to_retain,
        maximum_incremental_snapshot_archives_to_retain,
        use_progress_bar,
        &mut Some(Box::new(|download_progress: &DownloadProgressRecord| {
            debug!("Download progress: {download_progress:?}");
            if download_progress.last_throughput < minimal_snapshot_download_speed
                && download_progress.notification_count <= 1
                && download_progress.percentage_done <= 2_f32
                && download_progress.estimated_remaining_time > 60_f32
                && *download_abort_count < maximum_snapshot_download_abort
            {
                if let Some(ref known_validators) = validator_config.known_validators {
                    if known_validators.contains(rpc_contact_info.pubkey())
                        && known_validators.len() == 1
                        && bootstrap_config.only_known_rpc
                    {
                        warn!(
                            "The snapshot download is too slow, throughput: {} < min speed {} \
                            bytes/sec, but will NOT abort and try a different node as it is the \
                            only known validator and the --only-known-rpc flag is set. \
                            Abort count: {}, Progress detail: {:?}",
                            download_progress.last_throughput,
                            minimal_snapshot_download_speed,
                            download_abort_count,
                            download_progress,
                        );
                        return true; // Do not abort download from the one-and-only known validator
                    }
                }
                warn!(
                    "The snapshot download is too slow, throughput: {} < min speed {} \
                    bytes/sec, will abort and try a different node. \
                    Abort count: {}, Progress detail: {:?}",
                    download_progress.last_throughput,
                    minimal_snapshot_download_speed,
                    download_abort_count,
                    download_progress,
                );
                *download_abort_count += 1;
                false
            } else {
                true
            }
        })),
    )
}

/// Check to see if bootstrap should load from its local snapshots or not.  If not, then snapshots
/// will be downloaded.
fn should_use_local_snapshot(
    full_snapshot_archives_dir: &Path,
    incremental_snapshot_archives_dir: &Path,
    maximum_local_snapshot_age: Slot,
    full_snapshot_hash: (Slot, Hash),
    incremental_snapshot_hash: Option<(Slot, Hash)>,
    incremental_snapshot_fetch: bool,
) -> bool {
    let cluster_snapshot_slot = incremental_snapshot_hash
        .map(|(slot, _)| slot)
        .unwrap_or(full_snapshot_hash.0);

    match get_highest_local_snapshot_hash(
        full_snapshot_archives_dir,
        incremental_snapshot_archives_dir,
        incremental_snapshot_fetch,
    ) {
        None => {
            info!("Downloading a snapshot for slot {cluster_snapshot_slot} since there is not a local snapshot.");
            false
        }
        Some((local_snapshot_slot, _)) => {
            if local_snapshot_slot
                >= cluster_snapshot_slot.saturating_sub(maximum_local_snapshot_age)
            {
                info!("Reusing local snapshot at slot {local_snapshot_slot} instead of downloading a snapshot for slot {cluster_snapshot_slot}.");
                true
            } else {
                info!("Local snapshot from slot {local_snapshot_slot} is too old. Downloading a newer snapshot for slot {cluster_snapshot_slot}.");
                false
            }
        }
    }
}

/// Get the node's highest snapshot hashes from CRDS
fn get_snapshot_hashes_for_node(cluster_info: &ClusterInfo, node: &Pubkey) -> Option<SnapshotHash> {
    cluster_info.get_snapshot_hashes_for_node(node).map(
        |crds_value::SnapshotHashes {
             full, incremental, ..
         }| {
            let highest_incremental_snapshot_hash = incremental.into_iter().max();
            SnapshotHash {
                full,
                incr: highest_incremental_snapshot_hash,
            }
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    impl PeerSnapshotHash {
        fn new(
            rpc_contact_info: ContactInfo,
            full_snapshot_hash: (Slot, Hash),
            incremental_snapshot_hash: Option<(Slot, Hash)>,
        ) -> Self {
            Self {
                rpc_contact_info,
                snapshot_hash: SnapshotHash {
                    full: full_snapshot_hash,
                    incr: incremental_snapshot_hash,
                },
            }
        }
    }

    fn default_contact_info_for_tests() -> ContactInfo {
        ContactInfo::new_localhost(&Pubkey::default(), /*now:*/ 1_681_834_947_321)
    }

    #[test]
    fn test_build_known_snapshot_hashes() {
        solana_logger::setup();
        let full_snapshot_hash1 = (400_000, Hash::new_unique());
        let full_snapshot_hash2 = (400_000, Hash::new_unique());

        let incremental_snapshot_hash1 = (400_800, Hash::new_unique());
        let incremental_snapshot_hash2 = (400_800, Hash::new_unique());

        // simulate a set of known validators with various snapshot hashes
        let oracle = {
            let mut oracle = HashMap::new();

            for (full, incr) in [
                // only a full snapshot
                (full_snapshot_hash1, None),
                // full and incremental snapshots
                (full_snapshot_hash1, Some(incremental_snapshot_hash1)),
                // full and incremental snapshots, with different incremental hash
                (full_snapshot_hash1, Some(incremental_snapshot_hash2)),
                // ...and now with different full hashes
                (full_snapshot_hash2, None),
                (full_snapshot_hash2, Some(incremental_snapshot_hash1)),
                (full_snapshot_hash2, Some(incremental_snapshot_hash2)),
            ] {
                // also simulate multiple known validators having the same snapshot hashes
                oracle.insert(Pubkey::new_unique(), Some(SnapshotHash { full, incr }));
                oracle.insert(Pubkey::new_unique(), Some(SnapshotHash { full, incr }));
                oracle.insert(Pubkey::new_unique(), Some(SnapshotHash { full, incr }));
            }

            // no snapshots at all
            oracle.insert(Pubkey::new_unique(), None);
            oracle.insert(Pubkey::new_unique(), None);
            oracle.insert(Pubkey::new_unique(), None);

            oracle
        };

        let node_to_snapshot_hashes = |node| *oracle.get(node).unwrap();

        let known_snapshot_hashes =
            build_known_snapshot_hashes(oracle.keys(), node_to_snapshot_hashes);

        // ensure there's only one full snapshot hash, since they all used the same slot and there
        // can be only one snapshot hash per slot
        let known_full_snapshot_hashes = known_snapshot_hashes.keys();
        assert_eq!(known_full_snapshot_hashes.len(), 1);
        let known_full_snapshot_hash = known_full_snapshot_hashes.into_iter().next().unwrap();

        // and for the same reasons, ensure there is only one incremental snapshot hash
        let known_incremental_snapshot_hashes =
            known_snapshot_hashes.get(known_full_snapshot_hash).unwrap();
        assert_eq!(known_incremental_snapshot_hashes.len(), 1);
        let known_incremental_snapshot_hash =
            known_incremental_snapshot_hashes.iter().next().unwrap();

        // The resulting `known_snapshot_hashes` can be different from run-to-run due to how
        // `oracle.keys()` returns nodes during iteration.  Because of that, we cannot just assert
        // the full and incremental snapshot hashes are `full_snapshot_hash1` and
        // `incremental_snapshot_hash1`.  Instead, we assert that the full and incremental
        // snapshot hashes are exactly one or the other, since it depends on which nodes are seen
        // "first" when building the known snapshot hashes.
        assert!(
            known_full_snapshot_hash == &full_snapshot_hash1
                || known_full_snapshot_hash == &full_snapshot_hash2
        );
        assert!(
            known_incremental_snapshot_hash == &incremental_snapshot_hash1
                || known_incremental_snapshot_hash == &incremental_snapshot_hash2
        );
    }

    #[test]
    fn test_retain_peer_snapshot_hashes_that_match_known_snapshot_hashes() {
        let known_snapshot_hashes: KnownSnapshotHashes = [
            (
                (200_000, Hash::new_unique()),
                [
                    (200_200, Hash::new_unique()),
                    (200_400, Hash::new_unique()),
                    (200_600, Hash::new_unique()),
                    (200_800, Hash::new_unique()),
                ]
                .iter()
                .cloned()
                .collect(),
            ),
            (
                (300_000, Hash::new_unique()),
                [
                    (300_200, Hash::new_unique()),
                    (300_400, Hash::new_unique()),
                    (300_600, Hash::new_unique()),
                ]
                .iter()
                .cloned()
                .collect(),
            ),
        ]
        .iter()
        .cloned()
        .collect();

        let known_snapshot_hash = known_snapshot_hashes.iter().next().unwrap();
        let known_full_snapshot_hash = known_snapshot_hash.0;
        let known_incremental_snapshot_hash = known_snapshot_hash.1.iter().next().unwrap();

        let contact_info = default_contact_info_for_tests();
        let peer_snapshot_hashes = vec![
            // bad full snapshot hash, no incremental snapshot hash
            PeerSnapshotHash::new(contact_info.clone(), (111_000, Hash::default()), None),
            // bad everything
            PeerSnapshotHash::new(
                contact_info.clone(),
                (111_000, Hash::default()),
                Some((111_111, Hash::default())),
            ),
            // good full snapshot hash, no incremental snapshot hash
            PeerSnapshotHash::new(contact_info.clone(), *known_full_snapshot_hash, None),
            // bad full snapshot hash, good (not possible) incremental snapshot hash
            PeerSnapshotHash::new(
                contact_info.clone(),
                (111_000, Hash::default()),
                Some(*known_incremental_snapshot_hash),
            ),
            // good full snapshot hash, bad incremental snapshot hash
            PeerSnapshotHash::new(
                contact_info.clone(),
                *known_full_snapshot_hash,
                Some((111_111, Hash::default())),
            ),
            // good everything
            PeerSnapshotHash::new(
                contact_info.clone(),
                *known_full_snapshot_hash,
                Some(*known_incremental_snapshot_hash),
            ),
        ];

        let expected = vec![
            PeerSnapshotHash::new(contact_info.clone(), *known_full_snapshot_hash, None),
            PeerSnapshotHash::new(
                contact_info,
                *known_full_snapshot_hash,
                Some(*known_incremental_snapshot_hash),
            ),
        ];
        let mut actual = peer_snapshot_hashes;
        retain_peer_snapshot_hashes_that_match_known_snapshot_hashes(
            &known_snapshot_hashes,
            &mut actual,
        );
        assert_eq!(expected, actual);
    }

    #[test]
    fn test_retain_peer_snapshot_hashes_with_highest_full_snapshot_slot() {
        let contact_info = default_contact_info_for_tests();
        let peer_snapshot_hashes = vec![
            // old
            PeerSnapshotHash::new(contact_info.clone(), (100_000, Hash::default()), None),
            PeerSnapshotHash::new(
                contact_info.clone(),
                (100_000, Hash::default()),
                Some((100_100, Hash::default())),
            ),
            PeerSnapshotHash::new(
                contact_info.clone(),
                (100_000, Hash::default()),
                Some((100_200, Hash::default())),
            ),
            PeerSnapshotHash::new(
                contact_info.clone(),
                (100_000, Hash::default()),
                Some((100_300, Hash::default())),
            ),
            // new
            PeerSnapshotHash::new(contact_info.clone(), (200_000, Hash::default()), None),
            PeerSnapshotHash::new(
                contact_info.clone(),
                (200_000, Hash::default()),
                Some((200_100, Hash::default())),
            ),
            PeerSnapshotHash::new(
                contact_info.clone(),
                (200_000, Hash::default()),
                Some((200_200, Hash::default())),
            ),
            PeerSnapshotHash::new(
                contact_info.clone(),
                (200_000, Hash::default()),
                Some((200_300, Hash::default())),
            ),
        ];

        let expected = vec![
            PeerSnapshotHash::new(contact_info.clone(), (200_000, Hash::default()), None),
            PeerSnapshotHash::new(
                contact_info.clone(),
                (200_000, Hash::default()),
                Some((200_100, Hash::default())),
            ),
            PeerSnapshotHash::new(
                contact_info.clone(),
                (200_000, Hash::default()),
                Some((200_200, Hash::default())),
            ),
            PeerSnapshotHash::new(
                contact_info,
                (200_000, Hash::default()),
                Some((200_300, Hash::default())),
            ),
        ];
        let mut actual = peer_snapshot_hashes;
        retain_peer_snapshot_hashes_with_highest_full_snapshot_slot(&mut actual);
        assert_eq!(expected, actual);
    }

    #[test]
    fn test_retain_peer_snapshot_hashes_with_highest_incremental_snapshot_slot_some() {
        let contact_info = default_contact_info_for_tests();
        let peer_snapshot_hashes = vec![
            PeerSnapshotHash::new(contact_info.clone(), (200_000, Hash::default()), None),
            PeerSnapshotHash::new(
                contact_info.clone(),
                (200_000, Hash::default()),
                Some((200_100, Hash::default())),
            ),
            PeerSnapshotHash::new(
                contact_info.clone(),
                (200_000, Hash::default()),
                Some((200_200, Hash::default())),
            ),
            PeerSnapshotHash::new(
                contact_info.clone(),
                (200_000, Hash::default()),
                Some((200_300, Hash::default())),
            ),
            PeerSnapshotHash::new(
                contact_info.clone(),
                (200_000, Hash::default()),
                Some((200_010, Hash::default())),
            ),
            PeerSnapshotHash::new(
                contact_info.clone(),
                (200_000, Hash::default()),
                Some((200_020, Hash::default())),
            ),
            PeerSnapshotHash::new(
                contact_info.clone(),
                (200_000, Hash::default()),
                Some((200_030, Hash::default())),
            ),
        ];

        let expected = vec![PeerSnapshotHash::new(
            contact_info,
            (200_000, Hash::default()),
            Some((200_300, Hash::default())),
        )];
        let mut actual = peer_snapshot_hashes;
        retain_peer_snapshot_hashes_with_highest_incremental_snapshot_slot(&mut actual);
        assert_eq!(expected, actual);
    }

    /// Ensure that retaining the highest incremental snapshot hashes works as expected even if
    /// there are *zero* peers with incremental snapshots.
    #[test]
    fn test_retain_peer_snapshot_hashes_with_highest_incremental_snapshot_slot_none() {
        let contact_info = default_contact_info_for_tests();
        let peer_snapshot_hashes = vec![
            PeerSnapshotHash::new(contact_info.clone(), (200_000, Hash::new_unique()), None),
            PeerSnapshotHash::new(contact_info.clone(), (200_000, Hash::new_unique()), None),
            PeerSnapshotHash::new(contact_info, (200_000, Hash::new_unique()), None),
        ];

        let expected = peer_snapshot_hashes.clone();
        let mut actual = peer_snapshot_hashes;
        retain_peer_snapshot_hashes_with_highest_incremental_snapshot_slot(&mut actual);
        assert_eq!(expected, actual);
    }

    /// Ensure that retaining the highest snapshot hashes works (i.e. doesn't crash) even if the
    /// peer snapshot hashes input is empty.
    #[test]
    fn test_retain_peer_snapshot_hashes_with_highest_slot_empty() {
        {
            let mut actual = vec![];
            let expected = actual.clone();
            retain_peer_snapshot_hashes_with_highest_full_snapshot_slot(&mut actual);
            assert_eq!(expected, actual);
        }
        {
            let mut actual = vec![];
            let expected = actual.clone();
            retain_peer_snapshot_hashes_with_highest_incremental_snapshot_slot(&mut actual);
            assert_eq!(expected, actual);
        }
    }
}
