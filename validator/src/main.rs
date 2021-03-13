#![allow(clippy::integer_arithmetic)]
use {
    clap::{
        crate_description, crate_name, value_t, value_t_or_exit, values_t, values_t_or_exit, App,
        AppSettings, Arg, ArgMatches, SubCommand,
    },
    console::style,
    fd_lock::FdLock,
    log::*,
    rand::{seq::SliceRandom, thread_rng, Rng},
    solana_clap_utils::{
        input_parsers::{keypair_of, keypairs_of, pubkey_of, value_of},
        input_validators::{
            is_keypair_or_ask_keyword, is_parsable, is_pubkey, is_pubkey_or_keypair, is_slot,
        },
        keypair::SKIP_SEED_PHRASE_VALIDATION_ARG,
    },
    solana_client::{rpc_client::RpcClient, rpc_request::MAX_MULTIPLE_ACCOUNTS},
    solana_core::ledger_cleanup_service::{
        DEFAULT_MAX_LEDGER_SHREDS, DEFAULT_MIN_MAX_LEDGER_SHREDS,
    },
    solana_core::{
        cluster_info::{
            ClusterInfo, Node, MINIMUM_VALIDATOR_PORT_RANGE_WIDTH, VALIDATOR_PORT_RANGE,
        },
        contact_info::ContactInfo,
        gossip_service::GossipService,
        poh_service,
        rpc::JsonRpcConfig,
        rpc_pubsub_service::PubSubConfig,
        tpu::DEFAULT_TPU_COALESCE_MS,
        validator::{
            is_snapshot_config_invalid, Validator, ValidatorConfig, ValidatorStartProgress,
        },
    },
    solana_download_utils::{download_genesis_if_missing, download_snapshot},
    solana_ledger::blockstore_db::BlockstoreRecoveryMode,
    solana_perf::recycler::enable_recycler_warming,
    solana_runtime::{
        accounts_index::AccountIndex,
        bank_forks::{ArchiveFormat, SnapshotConfig, SnapshotVersion},
        hardened_unpack::{unpack_genesis_archive, MAX_GENESIS_ARCHIVE_UNPACKED_SIZE},
        snapshot_utils::get_highest_snapshot_archive_path,
    },
    solana_sdk::{
        clock::{Slot, DEFAULT_S_PER_SLOT},
        commitment_config::CommitmentConfig,
        genesis_config::GenesisConfig,
        hash::Hash,
        pubkey::Pubkey,
        signature::{Keypair, Signer},
    },
    solana_validator::{
        admin_rpc_service, dashboard::Dashboard, new_spinner_progress_bar, println_name_value,
        redirect_stderr_to_file,
    },
    std::{
        collections::{HashSet, VecDeque},
        env,
        fs::{self, File},
        net::{IpAddr, SocketAddr, TcpListener, UdpSocket},
        path::{Path, PathBuf},
        process::exit,
        str::FromStr,
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc, RwLock,
        },
        thread::sleep,
        time::{Duration, Instant, SystemTime},
    },
};

#[derive(Debug, PartialEq)]
enum Operation {
    Initialize,
    Run,
}

fn monitor_validator(ledger_path: &Path) {
    let dashboard = Dashboard::new(ledger_path, None, None).unwrap_or_else(|err| {
        println!(
            "Error: Unable to connect to validator at {}: {:?}",
            ledger_path.display(),
            err,
        );
        exit(1);
    });
    dashboard.run(Duration::from_secs(2));
}

fn wait_for_restart_window(
    ledger_path: &Path,
    min_idle_time_in_minutes: usize,
) -> Result<(), Box<dyn std::error::Error>> {
    let sleep_interval = Duration::from_secs(5);
    let min_delinquency_percentage = 0.05;

    let min_idle_slots = (min_idle_time_in_minutes as f64 * 60. / DEFAULT_S_PER_SLOT) as Slot;

    let admin_client = admin_rpc_service::connect(&ledger_path);
    let rpc_addr = admin_rpc_service::runtime()
        .block_on(async move { admin_client.await?.rpc_addr().await })
        .map_err(|err| format!("Unable to get validator RPC address: {}", err))?;

    let rpc_client = match rpc_addr {
        None => return Err("RPC not available".into()),
        Some(rpc_addr) => RpcClient::new_socket(rpc_addr),
    };

    let identity = rpc_client.get_identity()?;
    println_name_value("Identity:", &identity.to_string());
    println_name_value(
        "Minimum Idle Time:",
        &format!(
            "{} slots (~{} minutes)",
            min_idle_slots, min_idle_time_in_minutes
        ),
    );

    let mut current_epoch = None;
    let mut leader_schedule = VecDeque::new();
    let mut restart_snapshot = None;

    let progress_bar = new_spinner_progress_bar();
    let monitor_start_time = SystemTime::now();
    loop {
        let snapshot_slot = rpc_client.get_snapshot_slot().ok();
        let epoch_info = rpc_client.get_epoch_info_with_commitment(CommitmentConfig::processed())?;
        let healthy = rpc_client.get_health().ok().is_some();
        let delinquent_stake_percentage = {
            let vote_accounts = rpc_client.get_vote_accounts()?;
            let current_stake: u64 = vote_accounts
                .current
                .iter()
                .map(|va| va.activated_stake)
                .sum();
            let delinquent_stake: u64 = vote_accounts
                .delinquent
                .iter()
                .map(|va| va.activated_stake)
                .sum();
            let total_stake = current_stake + delinquent_stake;
            delinquent_stake as f64 / total_stake as f64
        };

        if match current_epoch {
            None => true,
            Some(current_epoch) => current_epoch != epoch_info.epoch,
        } {
            progress_bar.set_message(&format!(
                "Fetching leader schedule for epoch {}...",
                epoch_info.epoch
            ));
            let first_slot_in_epoch = epoch_info.absolute_slot - epoch_info.slot_index;
            leader_schedule = rpc_client
                .get_leader_schedule(Some(first_slot_in_epoch))?
                .ok_or_else(|| {
                    format!(
                        "Unable to get leader schedule from slot {}",
                        first_slot_in_epoch
                    )
                })?
                .get(&identity.to_string())
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .map(|slot_index| first_slot_in_epoch.saturating_add(slot_index as u64))
                .collect::<VecDeque<_>>();
            current_epoch = Some(epoch_info.epoch);
        }

        let status = {
            if !healthy {
                style("Node is unhealthy").red().to_string()
            } else {
                // Wait until a hole in the leader schedule before restarting the node
                let in_leader_schedule_hole =
                    if epoch_info.slot_index + min_idle_slots as u64 > epoch_info.slots_in_epoch {
                        Err("Current epoch is almost complete".to_string())
                    } else {
                        while leader_schedule
                            .get(0)
                            .map(|slot_index| *slot_index < epoch_info.absolute_slot)
                            .unwrap_or(false)
                        {
                            leader_schedule.pop_front();
                        }
                        match leader_schedule.get(0) {
                            None => {
                                Ok(()) // Validator has no leader slots
                            }
                            Some(next_leader_slot) => {
                                let idle_slots =
                                    next_leader_slot.saturating_sub(epoch_info.absolute_slot);
                                if idle_slots >= min_idle_slots {
                                    Ok(())
                                } else {
                                    Err(format!(
                                        "Validator will be leader soon. Next leader slot is {}",
                                        next_leader_slot
                                    ))
                                }
                            }
                        }
                    };

                match in_leader_schedule_hole {
                    Ok(_) => {
                        if restart_snapshot == None {
                            restart_snapshot = snapshot_slot;
                        }

                        if restart_snapshot == snapshot_slot {
                            "Waiting for a new snapshot".to_string()
                        } else if delinquent_stake_percentage >= min_delinquency_percentage {
                            style("Delinquency too high").red().to_string()
                        } else {
                            break; // Restart!
                        }
                    }
                    Err(why) => style(why).yellow().to_string(),
                }
            }
        };

        progress_bar.set_message(&format!(
            "{} | Processed Slot: {} | Snapshot Slot: {} | {:.2}% delinquent stake | {}",
            {
                let elapsed =
                    chrono::Duration::from_std(monitor_start_time.elapsed().unwrap()).unwrap();

                format!(
                    "{:02}:{:02}:{:02}",
                    elapsed.num_hours(),
                    elapsed.num_minutes() % 60,
                    elapsed.num_seconds() % 60
                )
            },
            epoch_info.absolute_slot,
            snapshot_slot
                .map(|s| s.to_string())
                .unwrap_or_else(|| "-".to_string()),
            delinquent_stake_percentage * 100.,
            status
        ));
        std::thread::sleep(sleep_interval);
    }
    drop(progress_bar);
    println!("{}", style("Ready to restart").green());
    Ok(())
}

fn port_range_validator(port_range: String) -> Result<(), String> {
    if let Some((start, end)) = solana_net_utils::parse_port_range(&port_range) {
        if end - start < MINIMUM_VALIDATOR_PORT_RANGE_WIDTH {
            Err(format!(
                "Port range is too small.  Try --dynamic-port-range {}-{}",
                start,
                start + MINIMUM_VALIDATOR_PORT_RANGE_WIDTH
            ))
        } else {
            Ok(())
        }
    } else {
        Err("Invalid port range".to_string())
    }
}

fn hash_validator(hash: String) -> Result<(), String> {
    Hash::from_str(&hash)
        .map(|_| ())
        .map_err(|e| format!("{:?}", e))
}

fn is_trusted_validator(id: &Pubkey, trusted_validators: &Option<HashSet<Pubkey>>) -> bool {
    if let Some(trusted_validators) = trusted_validators {
        trusted_validators.contains(id)
    } else {
        false
    }
}

fn get_trusted_snapshot_hashes(
    cluster_info: &ClusterInfo,
    trusted_validators: &Option<HashSet<Pubkey>>,
) -> Option<HashSet<(Slot, Hash)>> {
    if let Some(trusted_validators) = trusted_validators {
        let mut trusted_snapshot_hashes = HashSet::new();
        for trusted_validator in trusted_validators {
            cluster_info.get_snapshot_hash_for_node(trusted_validator, |snapshot_hashes| {
                for snapshot_hash in snapshot_hashes {
                    trusted_snapshot_hashes.insert(*snapshot_hash);
                }
            });
        }
        Some(trusted_snapshot_hashes)
    } else {
        None
    }
}

fn start_gossip_node(
    identity_keypair: &Arc<Keypair>,
    cluster_entrypoints: &[ContactInfo],
    ledger_path: &Path,
    gossip_addr: &SocketAddr,
    gossip_socket: UdpSocket,
    expected_shred_version: Option<u16>,
    gossip_validators: Option<HashSet<Pubkey>>,
    should_check_duplicate_instance: bool,
) -> (Arc<ClusterInfo>, Arc<AtomicBool>, GossipService) {
    let mut cluster_info = ClusterInfo::new(
        ClusterInfo::gossip_contact_info(
            &identity_keypair.pubkey(),
            *gossip_addr,
            expected_shred_version.unwrap_or(0),
        ),
        identity_keypair.clone(),
    );
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
        &gossip_exit_flag,
    );
    (cluster_info, gossip_exit_flag, gossip_service)
}

fn get_rpc_node(
    cluster_info: &ClusterInfo,
    cluster_entrypoints: &[ContactInfo],
    validator_config: &ValidatorConfig,
    blacklisted_rpc_nodes: &mut HashSet<Pubkey>,
    snapshot_not_required: bool,
    no_untrusted_rpc: bool,
    ledger_path: &std::path::Path,
) -> Option<(ContactInfo, Option<(Slot, Hash)>)> {
    let mut blacklist_timeout = Instant::now();
    let mut newer_cluster_snapshot_timeout = None;
    let mut retry_reason = None;
    loop {
        sleep(Duration::from_secs(1));
        info!("\n{}", cluster_info.rpc_info_trace());

        let shred_version = validator_config
            .expected_shred_version
            .unwrap_or_else(|| cluster_info.my_shred_version());
        if shred_version == 0 {
            let all_zero_shred_versions = cluster_entrypoints.iter().all(|cluster_entrypoint| {
                cluster_info
                    .lookup_contact_info_by_gossip_addr(&cluster_entrypoint.gossip)
                    .map_or(false, |entrypoint| entrypoint.shred_version == 0)
            });

            if all_zero_shred_versions {
                eprintln!(
                    "Entrypoint shred version is zero.  Restart with --expected-shred-version"
                );
                exit(1);
            }
            info!("Waiting to adopt entrypoint shred version...");
            continue;
        }

        info!(
            "Searching for an RPC service with shred version {}{}...",
            shred_version,
            retry_reason
                .as_ref()
                .map(|s| format!(" (Retrying: {})", s))
                .unwrap_or_default()
        );

        let rpc_peers = cluster_info
            .all_rpc_peers()
            .into_iter()
            .filter(|contact_info| contact_info.shred_version == shred_version)
            .collect::<Vec<_>>();
        let rpc_peers_total = rpc_peers.len();

        // Filter out blacklisted nodes
        let rpc_peers: Vec<_> = rpc_peers
            .into_iter()
            .filter(|rpc_peer| !blacklisted_rpc_nodes.contains(&rpc_peer.id))
            .collect();
        let rpc_peers_blacklisted = rpc_peers_total - rpc_peers.len();
        let rpc_peers_trusted = rpc_peers
            .iter()
            .filter(|rpc_peer| {
                is_trusted_validator(&rpc_peer.id, &validator_config.trusted_validators)
            })
            .count();

        info!(
            "Total {} RPC nodes found. {} trusted, {} blacklisted ",
            rpc_peers_total, rpc_peers_trusted, rpc_peers_blacklisted
        );

        if rpc_peers_blacklisted == rpc_peers_total {
            retry_reason = if !blacklisted_rpc_nodes.is_empty()
                && blacklist_timeout.elapsed().as_secs() > 60
            {
                // If all nodes are blacklisted and no additional nodes are discovered after 60 seconds,
                // remove the blacklist and try them all again
                blacklisted_rpc_nodes.clear();
                Some("Blacklist timeout expired".to_owned())
            } else {
                Some("Wait for trusted rpc peers".to_owned())
            };
            continue;
        }
        blacklist_timeout = Instant::now();

        let mut highest_snapshot_hash: Option<(Slot, Hash)> =
            get_highest_snapshot_archive_path(ledger_path)
                .map(|(_path, (slot, hash, _compression))| (slot, hash));
        let eligible_rpc_peers = if snapshot_not_required {
            rpc_peers
        } else {
            let trusted_snapshot_hashes =
                get_trusted_snapshot_hashes(&cluster_info, &validator_config.trusted_validators);

            let mut eligible_rpc_peers = vec![];

            for rpc_peer in rpc_peers.iter() {
                if no_untrusted_rpc
                    && !is_trusted_validator(&rpc_peer.id, &validator_config.trusted_validators)
                {
                    continue;
                }
                cluster_info.get_snapshot_hash_for_node(&rpc_peer.id, |snapshot_hashes| {
                    for snapshot_hash in snapshot_hashes {
                        if let Some(ref trusted_snapshot_hashes) = trusted_snapshot_hashes {
                            if !trusted_snapshot_hashes.contains(snapshot_hash) {
                                // Ignore all untrusted snapshot hashes
                                continue;
                            }
                        }

                        if highest_snapshot_hash.is_none()
                            || snapshot_hash.0 > highest_snapshot_hash.unwrap().0
                        {
                            // Found a higher snapshot, remove all nodes with a lower snapshot
                            eligible_rpc_peers.clear();
                            highest_snapshot_hash = Some(*snapshot_hash)
                        }

                        if Some(*snapshot_hash) == highest_snapshot_hash {
                            eligible_rpc_peers.push(rpc_peer.clone());
                        }
                    }
                });
            }

            match highest_snapshot_hash {
                None => {
                    assert!(eligible_rpc_peers.is_empty());
                }
                Some(highest_snapshot_hash) => {
                    if eligible_rpc_peers.is_empty() {
                        match newer_cluster_snapshot_timeout {
                            None => newer_cluster_snapshot_timeout = Some(Instant::now()),
                            Some(newer_cluster_snapshot_timeout) => {
                                if newer_cluster_snapshot_timeout.elapsed().as_secs() > 180 {
                                    warn!("giving up newer snapshot from the cluster");
                                    return None;
                                }
                            }
                        }
                        retry_reason = Some(format!(
                            "Wait for newer snapshot than local: {:?}",
                            highest_snapshot_hash
                        ));
                        continue;
                    }

                    info!(
                        "Highest available snapshot slot is {}, available from {} node{}: {:?}",
                        highest_snapshot_hash.0,
                        eligible_rpc_peers.len(),
                        if eligible_rpc_peers.len() > 1 {
                            "s"
                        } else {
                            ""
                        },
                        eligible_rpc_peers
                            .iter()
                            .map(|contact_info| contact_info.id)
                            .collect::<Vec<_>>()
                    );
                }
            }
            eligible_rpc_peers
        };

        if !eligible_rpc_peers.is_empty() {
            let contact_info =
                &eligible_rpc_peers[thread_rng().gen_range(0, eligible_rpc_peers.len())];
            return Some((contact_info.clone(), highest_snapshot_hash));
        } else {
            retry_reason = Some("No snapshots available".to_owned());
        }
    }
}

fn check_vote_account(
    rpc_client: &RpcClient,
    identity_pubkey: &Pubkey,
    vote_account_address: &Pubkey,
    authorized_voter_pubkeys: &[Pubkey],
) -> Result<(), String> {
    let vote_account = rpc_client
        .get_account_with_commitment(vote_account_address, CommitmentConfig::confirmed())
        .map_err(|err| format!("failed to fetch vote account: {}", err.to_string()))?
        .value
        .ok_or_else(|| format!("vote account does not exist: {}", vote_account_address))?;

    if vote_account.owner != solana_vote_program::id() {
        return Err(format!(
            "not a vote account (owned by {}): {}",
            vote_account.owner, vote_account_address
        ));
    }

    let identity_account = rpc_client
        .get_account_with_commitment(identity_pubkey, CommitmentConfig::confirmed())
        .map_err(|err| format!("failed to fetch identity account: {}", err.to_string()))?
        .value
        .ok_or_else(|| format!("identity account does not exist: {}", identity_pubkey))?;

    let vote_state = solana_vote_program::vote_state::VoteState::from(&vote_account);
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
            if !authorized_voter_pubkeys.contains(&vote_account_authorized_voter_pubkey) {
                return Err(format!(
                    "authorized voter {} not available",
                    vote_account_authorized_voter_pubkey
                ));
            }
        }
    } else {
        return Err(format!(
            "invalid vote account data for {}",
            vote_account_address
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

// This function is duplicated in ledger-tool/src/main.rs...
fn hardforks_of(matches: &ArgMatches<'_>, name: &str) -> Option<Vec<Slot>> {
    if matches.is_present(name) {
        Some(values_t_or_exit!(matches, name, Slot))
    } else {
        None
    }
}

fn validators_set(
    identity_pubkey: &Pubkey,
    matches: &ArgMatches<'_>,
    matches_name: &str,
    arg_name: &str,
) -> Option<HashSet<Pubkey>> {
    if matches.is_present(matches_name) {
        let validators_set: HashSet<_> = values_t_or_exit!(matches, matches_name, Pubkey)
            .into_iter()
            .collect();
        if validators_set.contains(identity_pubkey) {
            eprintln!(
                "The validator's identity pubkey cannot be a {}: {}",
                arg_name, identity_pubkey
            );
            exit(1);
        }
        Some(validators_set)
    } else {
        None
    }
}

fn check_genesis_hash(
    genesis_config: &GenesisConfig,
    expected_genesis_hash: Option<Hash>,
) -> Result<(), String> {
    let genesis_hash = genesis_config.hash();

    if let Some(expected_genesis_hash) = expected_genesis_hash {
        if expected_genesis_hash != genesis_hash {
            return Err(format!(
                "Genesis hash mismatch: expected {} but downloaded genesis hash is {}",
                expected_genesis_hash, genesis_hash,
            ));
        }
    }

    Ok(())
}

fn load_local_genesis(
    ledger_path: &std::path::Path,
    expected_genesis_hash: Option<Hash>,
) -> Result<GenesisConfig, String> {
    let existing_genesis = GenesisConfig::load(&ledger_path)
        .map_err(|err| format!("Failed to load genesis config: {}", err))?;
    check_genesis_hash(&existing_genesis, expected_genesis_hash)?;

    Ok(existing_genesis)
}

fn download_then_check_genesis_hash(
    rpc_addr: &SocketAddr,
    ledger_path: &std::path::Path,
    expected_genesis_hash: Option<Hash>,
    max_genesis_archive_unpacked_size: u64,
    no_genesis_fetch: bool,
    use_progress_bar: bool,
) -> Result<Hash, String> {
    if no_genesis_fetch {
        let genesis_config = load_local_genesis(ledger_path, expected_genesis_hash)?;
        return Ok(genesis_config.hash());
    }

    let genesis_package = ledger_path.join("genesis.tar.bz2");
    let genesis_config = if let Ok(tmp_genesis_package) =
        download_genesis_if_missing(rpc_addr, &genesis_package, use_progress_bar)
    {
        unpack_genesis_archive(
            &tmp_genesis_package,
            &ledger_path,
            max_genesis_archive_unpacked_size,
        )
        .map_err(|err| format!("Failed to unpack downloaded genesis config: {}", err))?;

        let downloaded_genesis = GenesisConfig::load(&ledger_path)
            .map_err(|err| format!("Failed to load downloaded genesis config: {}", err))?;

        check_genesis_hash(&downloaded_genesis, expected_genesis_hash)?;
        std::fs::rename(tmp_genesis_package, genesis_package)
            .map_err(|err| format!("Unable to rename: {:?}", err))?;

        downloaded_genesis
    } else {
        load_local_genesis(ledger_path, expected_genesis_hash)?
    };

    Ok(genesis_config.hash())
}

fn verify_reachable_ports(
    node: &Node,
    cluster_entrypoint: &ContactInfo,
    validator_config: &ValidatorConfig,
) -> bool {
    let mut udp_sockets = vec![&node.sockets.gossip, &node.sockets.repair];

    if ContactInfo::is_valid_address(&node.info.serve_repair) {
        udp_sockets.push(&node.sockets.serve_repair);
    }
    if ContactInfo::is_valid_address(&node.info.tpu) {
        udp_sockets.extend(node.sockets.tpu.iter());
    }
    if ContactInfo::is_valid_address(&node.info.tpu_forwards) {
        udp_sockets.extend(node.sockets.tpu_forwards.iter());
    }
    if ContactInfo::is_valid_address(&node.info.tvu) {
        udp_sockets.extend(node.sockets.tvu.iter());
        udp_sockets.extend(node.sockets.broadcast.iter());
        udp_sockets.extend(node.sockets.retransmit_sockets.iter());
    }
    if ContactInfo::is_valid_address(&node.info.tvu_forwards) {
        udp_sockets.extend(node.sockets.tvu_forwards.iter());
    }

    let mut tcp_listeners = vec![];
    if let Some((rpc_addr, rpc_pubsub_addr)) = validator_config.rpc_addrs {
        for (purpose, bind_addr, public_addr) in &[
            ("RPC", rpc_addr, &node.info.rpc),
            ("RPC pubsub", rpc_pubsub_addr, &node.info.rpc_pubsub),
        ] {
            if ContactInfo::is_valid_address(&public_addr) {
                tcp_listeners.push((
                    bind_addr.port(),
                    TcpListener::bind(bind_addr).unwrap_or_else(|err| {
                        error!(
                            "Unable to bind to tcp {:?} for {}: {}",
                            bind_addr, purpose, err
                        );
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
        &cluster_entrypoint.gossip,
        tcp_listeners,
        &udp_sockets,
    )
}

struct RpcBootstrapConfig {
    no_genesis_fetch: bool,
    no_snapshot_fetch: bool,
    no_untrusted_rpc: bool,
    max_genesis_archive_unpacked_size: u64,
    no_check_vote_account: bool,
}

impl Default for RpcBootstrapConfig {
    fn default() -> Self {
        Self {
            no_genesis_fetch: true,
            no_snapshot_fetch: true,
            no_untrusted_rpc: true,
            max_genesis_archive_unpacked_size: MAX_GENESIS_ARCHIVE_UNPACKED_SIZE,
            no_check_vote_account: true,
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn rpc_bootstrap(
    node: &Node,
    identity_keypair: &Arc<Keypair>,
    ledger_path: &Path,
    vote_account: &Pubkey,
    authorized_voter_keypairs: &[Arc<Keypair>],
    cluster_entrypoints: &[ContactInfo],
    validator_config: &mut ValidatorConfig,
    bootstrap_config: RpcBootstrapConfig,
    no_port_check: bool,
    use_progress_bar: bool,
    maximum_local_snapshot_age: Slot,
    should_check_duplicate_instance: bool,
    start_progress: &Arc<RwLock<ValidatorStartProgress>>,
) {
    if !no_port_check {
        let mut order: Vec<_> = (0..cluster_entrypoints.len()).collect();
        order.shuffle(&mut thread_rng());
        if order
            .into_iter()
            .all(|i| !verify_reachable_ports(&node, &cluster_entrypoints[i], &validator_config))
        {
            exit(1);
        }
    }

    if bootstrap_config.no_genesis_fetch && bootstrap_config.no_snapshot_fetch {
        return;
    }

    let mut blacklisted_rpc_nodes = HashSet::new();
    let mut gossip = None;
    loop {
        if gossip.is_none() {
            *start_progress.write().unwrap() = ValidatorStartProgress::SearchingForRpcService;

            gossip = Some(start_gossip_node(
                &identity_keypair,
                &cluster_entrypoints,
                ledger_path,
                &node.info.gossip,
                node.sockets.gossip.try_clone().unwrap(),
                validator_config.expected_shred_version,
                validator_config.gossip_validators.clone(),
                should_check_duplicate_instance,
            ));
        }

        let rpc_node_details = get_rpc_node(
            &gossip.as_ref().unwrap().0,
            &cluster_entrypoints,
            &validator_config,
            &mut blacklisted_rpc_nodes,
            bootstrap_config.no_snapshot_fetch,
            bootstrap_config.no_untrusted_rpc,
            ledger_path,
        );
        if rpc_node_details.is_none() {
            return;
        }
        let (rpc_contact_info, snapshot_hash) = rpc_node_details.unwrap();

        info!(
            "Using RPC service from node {}: {:?}",
            rpc_contact_info.id, rpc_contact_info.rpc
        );
        let rpc_client = RpcClient::new_socket(rpc_contact_info.rpc);

        let result = match rpc_client.get_version() {
            Ok(rpc_version) => {
                info!("RPC node version: {}", rpc_version.solana_core);
                Ok(())
            }
            Err(err) => Err(format!("Failed to get RPC node version: {}", err)),
        }
        .and_then(|_| {
            let genesis_hash = download_then_check_genesis_hash(
                &rpc_contact_info.rpc,
                &ledger_path,
                validator_config.expected_genesis_hash,
                bootstrap_config.max_genesis_archive_unpacked_size,
                bootstrap_config.no_genesis_fetch,
                use_progress_bar,
            );

            if let Ok(genesis_hash) = genesis_hash {
                if validator_config.expected_genesis_hash.is_none() {
                    info!("Expected genesis hash set to {}", genesis_hash);
                    validator_config.expected_genesis_hash = Some(genesis_hash);
                }
            }

            if let Some(expected_genesis_hash) = validator_config.expected_genesis_hash {
                // Sanity check that the RPC node is using the expected genesis hash before
                // downloading a snapshot from it
                let rpc_genesis_hash = rpc_client
                    .get_genesis_hash()
                    .map_err(|err| format!("Failed to get genesis hash: {}", err))?;

                if expected_genesis_hash != rpc_genesis_hash {
                    return Err(format!(
                        "Genesis hash mismatch: expected {} but RPC node genesis hash is {}",
                        expected_genesis_hash, rpc_genesis_hash
                    ));
                }
            }

            if let Some(snapshot_hash) = snapshot_hash {
                let mut use_local_snapshot = false;

                if let Some(highest_local_snapshot_slot) =
                    get_highest_snapshot_archive_path(ledger_path)
                        .map(|(_path, (slot, _hash, _compression))| slot)
                {
                    if highest_local_snapshot_slot
                        > snapshot_hash.0.saturating_sub(maximum_local_snapshot_age)
                    {
                        info!(
                            "Reusing local snapshot at slot {} instead \
                               of downloading a snapshot for slot {}",
                            highest_local_snapshot_slot, snapshot_hash.0
                        );
                        use_local_snapshot = true;
                    } else {
                        info!(
                            "Local snapshot from slot {} is too old. \
                              Downloading a newer snapshot for slot {}",
                            highest_local_snapshot_slot, snapshot_hash.0
                        );
                    }
                }

                if use_local_snapshot {
                    Ok(())
                } else {
                    rpc_client
                        .get_slot_with_commitment(CommitmentConfig::finalized())
                        .map_err(|err| format!("Failed to get RPC node slot: {}", err))
                        .and_then(|slot| {
                            *start_progress.write().unwrap() =
                                ValidatorStartProgress::DownloadingSnapshot {
                                    slot: snapshot_hash.0,
                                    rpc_addr: rpc_contact_info.rpc,
                                };
                            info!("RPC node root slot: {}", slot);
                            let (cluster_info, gossip_exit_flag, gossip_service) =
                                gossip.take().unwrap();
                            cluster_info.save_contact_info();
                            gossip_exit_flag.store(true, Ordering::Relaxed);
                            let ret = download_snapshot(
                                &rpc_contact_info.rpc,
                                &ledger_path,
                                snapshot_hash,
                                use_progress_bar,
                            );
                            gossip_service.join().unwrap();
                            ret
                        })
                }
            } else {
                Ok(())
            }
        })
        .map(|_| {
            if !validator_config.voting_disabled && !bootstrap_config.no_check_vote_account {
                check_vote_account(
                    &rpc_client,
                    &identity_keypair.pubkey(),
                    &vote_account,
                    &authorized_voter_keypairs
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
                    error!("{}", err);
                    exit(1);
                });
            }
        });

        if result.is_ok() {
            break;
        }
        warn!("{}", result.unwrap_err());

        if let Some(ref trusted_validators) = validator_config.trusted_validators {
            if trusted_validators.contains(&rpc_contact_info.id) {
                continue; // Never blacklist a trusted node
            }
        }

        info!(
            "Excluding {} as a future RPC candidate",
            rpc_contact_info.id
        );
        blacklisted_rpc_nodes.insert(rpc_contact_info.id);
    }
    if let Some((cluster_info, gossip_exit_flag, gossip_service)) = gossip.take() {
        cluster_info.save_contact_info();
        gossip_exit_flag.store(true, Ordering::Relaxed);
        gossip_service.join().unwrap();
    }
}

pub fn main() {
    let default_dynamic_port_range =
        &format!("{}-{}", VALIDATOR_PORT_RANGE.0, VALIDATOR_PORT_RANGE.1);
    let default_genesis_archive_unpacked_size = &MAX_GENESIS_ARCHIVE_UNPACKED_SIZE.to_string();
    let default_rpc_max_multiple_accounts = &MAX_MULTIPLE_ACCOUNTS.to_string();
    let default_rpc_pubsub_max_connections = PubSubConfig::default().max_connections.to_string();
    let default_rpc_pubsub_max_fragment_size =
        PubSubConfig::default().max_fragment_size.to_string();
    let default_rpc_pubsub_max_in_buffer_capacity =
        PubSubConfig::default().max_in_buffer_capacity.to_string();
    let default_rpc_pubsub_max_out_buffer_capacity =
        PubSubConfig::default().max_out_buffer_capacity.to_string();
    let default_rpc_send_transaction_retry_ms = ValidatorConfig::default()
        .send_transaction_retry_ms
        .to_string();
    let default_rpc_send_transaction_leader_forward_count = ValidatorConfig::default()
        .send_transaction_leader_forward_count
        .to_string();
    let default_rpc_threads = num_cpus::get().to_string();

    let matches = App::new(crate_name!()).about(crate_description!())
        .version(solana_version::version!())
        .setting(AppSettings::VersionlessSubcommands)
        .setting(AppSettings::InferSubcommands)
        .arg(
            Arg::with_name(SKIP_SEED_PHRASE_VALIDATION_ARG.name)
                .long(SKIP_SEED_PHRASE_VALIDATION_ARG.long)
                .help(SKIP_SEED_PHRASE_VALIDATION_ARG.help),
        )
        .arg(
            Arg::with_name("identity")
                .short("i")
                .long("identity")
                .value_name("KEYPAIR")
                .takes_value(true)
                .validator(is_keypair_or_ask_keyword)
                .help("Validator identity keypair"),
        )
        .arg(
            Arg::with_name("authorized_voter_keypairs")
                .long("authorized-voter")
                .value_name("KEYPAIR")
                .takes_value(true)
                .validator(is_keypair_or_ask_keyword)
                .requires("vote_account")
                .multiple(true)
                .help("Include an additional authorized voter keypair. \
                       May be specified multiple times. \
                       [default: the --identity keypair]"),
        )
        .arg(
            Arg::with_name("vote_account")
                .long("vote-account")
                .value_name("ADDRESS")
                .takes_value(true)
                .validator(is_pubkey_or_keypair)
                .requires("identity")
                .help("Validator vote account public key.  \
                       If unspecified voting will be disabled. \
                       The authorized voter for the account must either be the \
                       --identity keypair or with the --authorized-voter argument")
        )
        .arg(
            Arg::with_name("init_complete_file")
                .long("init-complete-file")
                .value_name("FILE")
                .takes_value(true)
                .help("Create this file if it doesn't already exist \
                       once validator initialization is complete"),
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
            Arg::with_name("no_snapshot_fetch")
                .long("no-snapshot-fetch")
                .takes_value(false)
                .help("Do not attempt to fetch a snapshot from the cluster, \
                      start from a local snapshot if present"),
        )
        .arg(
            Arg::with_name("no_genesis_fetch")
                .long("no-genesis-fetch")
                .takes_value(false)
                .help("Do not fetch genesis from the cluster"),
        )
        .arg(
            Arg::with_name("no_voting")
                .long("no-voting")
                .takes_value(false)
                .help("Launch validator without voting"),
        )
        .arg(
            Arg::with_name("no_check_vote_account")
                .long("no-check-vote-account")
                .takes_value(false)
                .conflicts_with("no_voting")
                .requires("entrypoint")
                .help("Skip the RPC vote account sanity check")
        )
        .arg(
            Arg::with_name("restricted_repair_only_mode")
                .long("restricted-repair-only-mode")
                .takes_value(false)
                .help("Do not publish the Gossip, TPU, TVU or Repair Service ports causing \
                       the validator to operate in a limited capacity that reduces its \
                       exposure to the rest of the cluster. \
                       \
                       The --no-voting flag is implicit when this flag is enabled \
                      "),
        )
        .arg(
            Arg::with_name("dev_halt_at_slot")
                .long("dev-halt-at-slot")
                .value_name("SLOT")
                .validator(is_slot)
                .takes_value(true)
                .help("Halt the validator when it reaches the given slot"),
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
            Arg::with_name("minimal_rpc_api")
                .long("--minimal-rpc-api")
                .takes_value(false)
                .help("Only expose the RPC methods required to serve snapshots to other nodes"),
        )
        .arg(
            Arg::with_name("private_rpc")
                .long("--private-rpc")
                .takes_value(false)
                .help("Do not publish the RPC port for use by others")
        )
        .arg(
            Arg::with_name("no_port_check")
                .long("--no-port-check")
                .takes_value(false)
                .help("Do not perform TCP/UDP reachable port checks at start-up")
        )
        .arg(
            Arg::with_name("enable_rpc_transaction_history")
                .long("enable-rpc-transaction-history")
                .takes_value(false)
                .help("Enable historical transaction info over JSON RPC, \
                       including the 'getConfirmedBlock' API.  \
                       This will cause an increase in disk usage and IOPS"),
        )
        .arg(
            Arg::with_name("enable_rpc_bigtable_ledger_storage")
                .long("enable-rpc-bigtable-ledger-storage")
                .requires("enable_rpc_transaction_history")
                .takes_value(false)
                .help("Fetch historical transaction info from a BigTable instance \
                       as a fallback to local ledger data"),
        )
        .arg(
            Arg::with_name("enable_bigtable_ledger_upload")
                .long("enable-bigtable-ledger-upload")
                .requires("enable_rpc_transaction_history")
                .takes_value(false)
                .help("Upload new confirmed blocks into a BigTable instance"),
        )
        .arg(
            Arg::with_name("enable_cpi_and_log_storage")
                .long("enable-cpi-and-log-storage")
                .requires("enable_rpc_transaction_history")
                .takes_value(false)
                .help("Include CPI inner instructions and logs in the \
                        historical transaction info stored"),
        )
        .arg(
            Arg::with_name("rpc_max_multiple_accounts")
                .long("rpc-max-multiple-accounts")
                .value_name("MAX ACCOUNTS")
                .takes_value(true)
                .default_value(default_rpc_max_multiple_accounts)
                .help("Override the default maximum accounts accepted by \
                       the getMultipleAccounts JSON RPC method")
        )
        .arg(
            Arg::with_name("health_check_slot_distance")
                .long("health-check-slot-distance")
                .value_name("SLOT_DISTANCE")
                .takes_value(true)
                .default_value("150")
                .help("If --trusted-validators are specified, report this validator healthy \
                       if its latest account hash is no further behind than this number of \
                       slots from the latest trusted validator account hash. \
                       If no --trusted-validators are specified, the validator will always \
                       report itself to be healthy")
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
            Arg::with_name("account_paths")
                .long("accounts")
                .value_name("PATHS")
                .takes_value(true)
                .multiple(true)
                .help("Comma separated persistent accounts location"),
        )
        .arg(
            Arg::with_name("account_shrink_path")
                .long("account-shrink-path")
                .value_name("PATH")
                .takes_value(true)
                .multiple(true)
                .help("Path to accounts shrink path which can hold a compacted account set."),
        )
        .arg(
            Arg::with_name("gossip_port")
                .long("gossip-port")
                .value_name("PORT")
                .takes_value(true)
                .help("Gossip port number for the validator"),
        )
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
            Arg::with_name("public_rpc_addr")
                .long("public-rpc-address")
                .value_name("HOST:PORT")
                .takes_value(true)
                .conflicts_with("private_rpc")
                .validator(solana_net_utils::is_host_port)
                .help("RPC address for the validator to advertise publicly in gossip. \
                      Useful for validators running behind a load balancer or proxy \
                      [default: use --rpc-bind-address / --rpc-port]"),
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
            Arg::with_name("maximum_local_snapshot_age")
                .long("maximum-local-snapshot-age")
                .value_name("NUMBER_OF_SLOTS")
                .takes_value(true)
                .default_value("500")
                .help("Reuse a local snapshot if it's less than this many \
                       slots behind the highest snapshot available for \
                       download from other validators"),
        )
        .arg(
            Arg::with_name("snapshot_interval_slots")
                .long("snapshot-interval-slots")
                .value_name("SNAPSHOT_INTERVAL_SLOTS")
                .takes_value(true)
                .default_value("100")
                .help("Number of slots between generating snapshots, \
                      0 to disable snapshots"),
        )
        .arg(
            Arg::with_name("contact_debug_interval")
                .long("contact-debug-interval")
                .value_name("CONTACT_DEBUG_INTERVAL")
                .takes_value(true)
                .default_value("10000")
                .help("Milliseconds between printing contact debug from gossip."),
        )
        .arg(
            Arg::with_name("no_poh_speed_test")
                .long("no-poh-speed-test")
                .help("Skip the check for PoH speed."),
        )
        .arg(
            Arg::with_name("accounts_hash_interval_slots")
                .long("accounts-hash-slots")
                .value_name("ACCOUNTS_HASH_INTERVAL_SLOTS")
                .takes_value(true)
                .default_value("100")
                .help("Number of slots between generating accounts hash."),
        )
        .arg(
            Arg::with_name("snapshot_version")
                .long("snapshot-version")
                .value_name("SNAPSHOT_VERSION")
                .validator(is_parsable::<SnapshotVersion>)
                .takes_value(true)
                .default_value(SnapshotVersion::default().into())
                .help("Output snapshot version"),
        )
        .arg(
            Arg::with_name("limit_ledger_size")
                .long("limit-ledger-size")
                .value_name("SHRED_COUNT")
                .takes_value(true)
                .min_values(0)
                .max_values(1)
                /* .default_value() intentionally not used here! */
                .help("Keep this amount of shreds in root slots."),
        )
        .arg(
            Arg::with_name("skip_poh_verify")
                .long("skip-poh-verify")
                .takes_value(false)
                .help("Skip ledger verification at validator bootup"),
        )
        .arg(
            Arg::with_name("cuda")
                .long("cuda")
                .takes_value(false)
                .help("Use CUDA"),
        )
        .arg(
            clap::Arg::with_name("require_tower")
                .long("require-tower")
                .takes_value(false)
                .help("Refuse to start if saved tower state is not found"),
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
            Arg::with_name("expected_bank_hash")
                .long("expected-bank-hash")
                .value_name("HASH")
                .takes_value(true)
                .validator(hash_validator)
                .help("When wait-for-supermajority <x>, require the bank at <x> to have this hash"),
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
                .help("Redirect logging to the specified file, '-' for standard error. \
                       Sending the SIGUSR1 signal to the validator process will cause it \
                       to re-open the log file"),
        )
        .arg(
            Arg::with_name("wait_for_supermajority")
                .long("wait-for-supermajority")
                .requires("expected_bank_hash")
                .value_name("SLOT")
                .validator(is_slot)
                .help("After processing the ledger and the next slot is SLOT, wait until a \
                       supermajority of stake is visible on gossip before starting PoH"),
        )
        .arg(
            Arg::with_name("hard_forks")
                .long("hard-fork")
                .value_name("SLOT")
                .validator(is_slot)
                .multiple(true)
                .takes_value(true)
                .help("Add a hard fork at this slot"),
        )
        .arg(
            Arg::with_name("trusted_validators")
                .long("trusted-validator")
                .validator(is_pubkey)
                .value_name("VALIDATOR IDENTITY")
                .multiple(true)
                .takes_value(true)
                .help("A snapshot hash must be published in gossip by this validator to be accepted. \
                       May be specified multiple times. If unspecified any snapshot hash will be accepted"),
        )
        .arg(
            Arg::with_name("debug_key")
                .long("debug-key")
                .validator(is_pubkey)
                .value_name("ADDRESS")
                .multiple(true)
                .takes_value(true)
                .help("Log when transactions are processed which reference a given key."),
        )
        .arg(
            Arg::with_name("no_untrusted_rpc")
                .long("no-untrusted-rpc")
                .takes_value(false)
                .help("Use the RPC service of trusted validators only")
        )
        .arg(
            Arg::with_name("repair_validators")
                .long("repair-validator")
                .validator(is_pubkey)
                .value_name("VALIDATOR IDENTITY")
                .multiple(true)
                .takes_value(true)
                .help("A list of validators to request repairs from. If specified, repair will not \
                       request from validators outside this set [default: all validators]")
        )
        .arg(
            Arg::with_name("gossip_validators")
                .long("gossip-validator")
                .validator(is_pubkey)
                .value_name("VALIDATOR IDENTITY")
                .multiple(true)
                .takes_value(true)
                .help("A list of validators to gossip with.  If specified, gossip \
                      will not pull/pull from from validators outside this set. \
                      [default: all validators]")
        )
        .arg(
            Arg::with_name("no_rocksdb_compaction")
                .long("no-rocksdb-compaction")
                .takes_value(false)
                .help("Disable manual compaction of the ledger database. May increase storage requirements.")
        )
        .arg(
            Arg::with_name("rocksdb_compaction_interval")
                .long("rocksdb-compaction-interval-slots")
                .value_name("ROCKSDB_COMPACTION_INTERVAL_SLOTS")
                .takes_value(true)
                .help("Number of slots between compacting ledger"),
        )
        .arg(
            Arg::with_name("tpu_coalesce_ms")
                .long("tpu-coalesce-ms")
                .value_name("MILLISECS")
                .takes_value(true)
                .validator(is_parsable::<u64>)
                .help("Milliseconds to wait in the TPU receiver for packet coalescing."),
        )
        .arg(
            Arg::with_name("rocksdb_max_compaction_jitter")
                .long("rocksdb-max-compaction-jitter-slots")
                .value_name("ROCKSDB_MAX_COMPACTION_JITTER_SLOTS")
                .takes_value(true)
                .help("Introduce jitter into the compaction to offset compaction operation"),
        )
        .arg(
            Arg::with_name("bind_address")
                .long("bind-address")
                .value_name("HOST")
                .takes_value(true)
                .validator(solana_net_utils::is_host)
                .default_value("0.0.0.0")
                .help("IP address to bind the validator ports"),
        )
        .arg(
            Arg::with_name("rpc_bind_address")
                .long("rpc-bind-address")
                .value_name("HOST")
                .takes_value(true)
                .validator(solana_net_utils::is_host)
                .help("IP address to bind the RPC port [default: use --bind-address]"),
        )
        .arg(
            Arg::with_name("rpc_threads")
                .long("rpc-threads")
                .value_name("NUMBER")
                .validator(is_parsable::<usize>)
                .takes_value(true)
                .default_value(&default_rpc_threads)
                .help("Number of threads to use for servicing RPC requests"),
        )
        .arg(
            Arg::with_name("rpc_bigtable_timeout")
                .long("rpc-bigtable-timeout")
                .value_name("SECONDS")
                .validator(is_parsable::<u64>)
                .takes_value(true)
                .default_value("30")
                .help("Number of seconds before timing out RPC requests backed by BigTable"),
        )
        .arg(
            Arg::with_name("rpc_pubsub_enable_vote_subscription")
                .long("rpc-pubsub-enable-vote-subscription")
                .takes_value(false)
                .help("Enable the unstable RPC PubSub `voteSubscribe` subscription"),
        )
        .arg(
            Arg::with_name("rpc_pubsub_max_connections")
                .long("rpc-pubsub-max-connections")
                .value_name("NUMBER")
                .takes_value(true)
                .validator(is_parsable::<usize>)
                .default_value(&default_rpc_pubsub_max_connections)
                .help("The maximum number of connections that RPC PubSub will support. \
                       This is a hard limit and no new connections beyond this limit can \
                       be made until an old connection is dropped."),
        )
        .arg(
            Arg::with_name("rpc_pubsub_max_fragment_size")
                .long("rpc-pubsub-max-fragment-size")
                .value_name("BYTES")
                .takes_value(true)
                .validator(is_parsable::<usize>)
                .default_value(&default_rpc_pubsub_max_fragment_size)
                .help("The maximum length in bytes of acceptable incoming frames. Messages longer \
                       than this will be rejected."),
        )
        .arg(
            Arg::with_name("rpc_pubsub_max_in_buffer_capacity")
                .long("rpc-pubsub-max-in-buffer-capacity")
                .value_name("BYTES")
                .takes_value(true)
                .validator(is_parsable::<usize>)
                .default_value(&default_rpc_pubsub_max_in_buffer_capacity)
                .help("The maximum size in bytes to which the incoming websocket buffer can grow."),
        )
        .arg(
            Arg::with_name("rpc_pubsub_max_out_buffer_capacity")
                .long("rpc-pubsub-max-out-buffer-capacity")
                .value_name("BYTES")
                .takes_value(true)
                .validator(is_parsable::<usize>)
                .default_value(&default_rpc_pubsub_max_out_buffer_capacity)
                .help("The maximum size in bytes to which the outgoing websocket buffer can grow."),
        )
        .arg(
            Arg::with_name("rpc_send_transaction_retry_ms")
                .long("rpc-send-retry-ms")
                .value_name("MILLISECS")
                .takes_value(true)
                .validator(is_parsable::<u64>)
                .default_value(&default_rpc_send_transaction_retry_ms)
                .help("The rate at which transactions sent via rpc service are retried."),
        )
        .arg(
            Arg::with_name("rpc_send_transaction_leader_forward_count")
                .long("rpc-send-leader-count")
                .value_name("NUMBER")
                .takes_value(true)
                .validator(is_parsable::<u64>)
                .default_value(&default_rpc_send_transaction_leader_forward_count)
                .help("The number of upcoming leaders to which to forward transactions sent via rpc service."),
        )
        .arg(
            Arg::with_name("halt_on_trusted_validators_accounts_hash_mismatch")
                .long("halt-on-trusted-validators-accounts-hash-mismatch")
                .requires("trusted_validators")
                .takes_value(false)
                .help("Abort the validator if a bank hash mismatch is detected within trusted validator set"),
        )
        .arg(
            Arg::with_name("frozen_accounts")
                .long("frozen-account")
                .validator(is_pubkey)
                .value_name("PUBKEY")
                .multiple(true)
                .takes_value(true)
                .help("Freeze the specified account.  This will cause the validator to \
                       intentionally crash should any transaction modify the frozen account in any way \
                       other than increasing the account balance"),
        )
        .arg(
            Arg::with_name("snapshot_archive_format")
                .long("snapshot-archive-format")
                .alias("snapshot-compression") // Legacy name used by Solana v1.5.x and older
                .possible_values(&["bz2", "gzip", "zstd", "tar", "none"])
                .default_value("zstd")
                .value_name("ARCHIVE_TYPE")
                .takes_value(true)
                .help("Snapshot archive format to use."),
        )
        .arg(
            Arg::with_name("max_genesis_archive_unpacked_size")
                .long("max-genesis-archive-unpacked-size")
                .value_name("NUMBER")
                .takes_value(true)
                .default_value(&default_genesis_archive_unpacked_size)
                .help(
                    "maximum total uncompressed file size of downloaded genesis archive",
                ),
        )
        .arg(
            Arg::with_name("wal_recovery_mode")
                .long("wal-recovery-mode")
                .value_name("MODE")
                .takes_value(true)
                .possible_values(&[
                    "tolerate_corrupted_tail_records",
                    "absolute_consistency",
                    "point_in_time",
                    "skip_any_corrupted_record"])
                .help(
                    "Mode to recovery the ledger db write ahead log."
                ),
        )
        .arg(
            Arg::with_name("no_bpf_jit")
                .long("no-bpf-jit")
                .takes_value(false)
                .help("Disable the just-in-time compiler and instead use the interpreter for BPF"),
        )
        .arg(
            // legacy nop argument
            Arg::with_name("bpf_jit")
                .long("bpf-jit")
                .hidden(true)
                .takes_value(false)
                .conflicts_with("no_bpf_jit")
        )
        .arg(
            Arg::with_name("poh_pinned_cpu_core")
                .hidden(true)
                .long("experimental-poh-pinned-cpu-core")
                .takes_value(true)
                .value_name("CPU_CORE_INDEX")
                .validator(|s| {
                    let core_index = usize::from_str(&s).map_err(|e| e.to_string())?;
                    let max_index = core_affinity::get_core_ids().map(|cids| cids.len() - 1).unwrap_or(0);
                    if core_index > max_index {
                        return Err(format!("core index must be in the range [0, {}]", max_index));
                    }
                    Ok(())
                })
                .help("EXPERIMENTAL: Specify which CPU core PoH is pinned to"),
        )
        .arg(
            Arg::with_name("poh_hashes_per_batch")
                .hidden(true)
                .long("poh-hashes-per-batch")
                .takes_value(true)
                .value_name("NUM")
                .help("Specify hashes per batch in PoH service"),
        )
        .arg(
            Arg::with_name("account_indexes")
                .long("account-index")
                .takes_value(true)
                .multiple(true)
                .possible_values(&["program-id", "spl-token-owner", "spl-token-mint"])
                .value_name("INDEX")
                .help("Enable an accounts index, indexed by the selected account field"),
        )
        .arg(
            Arg::with_name("no_accounts_db_caching")
                .long("no-accounts-db-caching")
                .help("Disables accounts caching"),
        )
        .arg(
            Arg::with_name("accounts_db_test_hash_calculation")
                .long("accounts-db-test-hash-calculation")
                .help("Enables testing of hash calculation using stores in AccountsHashVerifier. This has a computational cost."),
        )
        .arg(
            Arg::with_name("accounts_db_index_hashing")
                .long("accounts-db-index-hashing")
                .help("Enables the use of the index in hash calculation in AccountsHashVerifier/Accounts Background Service."),
        )
        .arg(
            Arg::with_name("no_accounts_db_index_hashing")
                .long("no-accounts-db-index-hashing")
                .help("This is obsolete. See --accounts-db-index-hashing. Disables the use of the index in hash calculation in AccountsHashVerifier/Accounts Background Service."),
        )
        .arg(
            // legacy nop argument
            Arg::with_name("accounts_db_caching_enabled")
                .long("accounts-db-caching-enabled")
                .conflicts_with("no_accounts_db_caching")
                .hidden(true)
        )
        .arg(
            Arg::with_name("no_duplicate_instance_check")
                .long("no-duplicate-instance-check")
                .takes_value(false)
                .help("Disables duplicate instance check")
                .hidden(true),
        )
        .after_help("The default subcommand is run")
        .subcommand(
             SubCommand::with_name("exit")
             .about("Send an exit request to the validator")
             .arg(
                 Arg::with_name("force")
                     .short("f")
                     .long("force")
                     .takes_value(false)
                     .help("Request the validator exit immediately instead of waiting for a restart window")
             )
             .arg(
                 Arg::with_name("monitor")
                     .short("m")
                     .long("monitor")
                     .takes_value(false)
                     .help("Monitor the validator after sending the exit request")
             )
             .arg(
                 Arg::with_name("min_idle_time")
                     .takes_value(true)
                     .long("min-idle-time")
                     .value_name("MINUTES")
                     .validator(is_parsable::<usize>)
                     .default_value("10")
                     .help("Minimum time that the validator should not be leader before restarting")
             )
         )
        .subcommand(
             SubCommand::with_name("init")
             .about("Initialize the ledger directory then exit")
         )
        .subcommand(
             SubCommand::with_name("monitor")
             .about("Monitor the validator")
         )
        .subcommand(
             SubCommand::with_name("run")
             .about("Run the validator")
         )
        .subcommand(
             SubCommand::with_name("set-log-filter")
             .about("Adjust the validator log filter")
             .arg(
                 Arg::with_name("filter")
                     .takes_value(true)
                     .index(1)
                     .help("New filter using the same format as the RUST_LOG environment variable")
             )
             .after_help("Note: the new filter only applies to the currently running validator instance")
         )
        .subcommand(
             SubCommand::with_name("wait-for-restart-window")
             .about("Monitor the validator for a good time to restart")
             .arg(
                 Arg::with_name("min_idle_time")
                     .takes_value(true)
                     .index(1)
                     .validator(is_parsable::<usize>)
                     .value_name("MINUTES")
                     .default_value("10")
                     .help("Minimum time that the validator should not be leader before restarting")
             )
             .after_help("Note: If this command exits with a non-zero status \
                          then this not a good time for a restart")
         )
        .get_matches();

    let ledger_path = PathBuf::from(matches.value_of("ledger_path").unwrap());

    let operation = match matches.subcommand() {
        ("", _) | ("run", _) => Operation::Run,
        ("init", _) => Operation::Initialize,
        ("exit", Some(subcommand_matches)) => {
            let min_idle_time = value_t_or_exit!(subcommand_matches, "min_idle_time", usize);
            let force = subcommand_matches.is_present("force");
            let monitor = subcommand_matches.is_present("monitor");

            if !force {
                wait_for_restart_window(&ledger_path, min_idle_time).unwrap_or_else(|err| {
                    println!("{}", err);
                    exit(1);
                });
            }

            let admin_client = admin_rpc_service::connect(&ledger_path);
            admin_rpc_service::runtime()
                .block_on(async move { admin_client.await?.exit().await })
                .unwrap_or_else(|err| {
                    println!("exit request failed: {}", err);
                    exit(1);
                });
            println!("Exit request sent");

            if monitor {
                monitor_validator(&ledger_path);
            }
            return;
        }
        ("monitor", _) => {
            monitor_validator(&ledger_path);
            return;
        }
        ("set-log-filter", Some(subcommand_matches)) => {
            let filter = value_t_or_exit!(subcommand_matches, "filter", String);
            let admin_client = admin_rpc_service::connect(&ledger_path);
            admin_rpc_service::runtime()
                .block_on(async move { admin_client.await?.set_log_filter(filter).await })
                .unwrap_or_else(|err| {
                    println!("set log filter failed: {}", err);
                    exit(1);
                });
            return;
        }
        ("wait-for-restart-window", Some(subcommand_matches)) => {
            let min_idle_time = value_t_or_exit!(subcommand_matches, "min_idle_time", usize);
            wait_for_restart_window(&ledger_path, min_idle_time).unwrap_or_else(|err| {
                println!("{}", err);
                exit(1);
            });
            return;
        }
        _ => unreachable!(),
    };

    let identity_keypair = Arc::new(keypair_of(&matches, "identity").unwrap_or_else(|| {
        clap::Error::with_description(
            "The --identity <KEYPAIR> argument is required",
            clap::ErrorKind::ArgumentNotFound,
        )
        .exit();
    }));

    let authorized_voter_keypairs = keypairs_of(&matches, "authorized_voter_keypairs")
        .map(|keypairs| keypairs.into_iter().map(Arc::new).collect())
        .unwrap_or_else(|| vec![identity_keypair.clone()]);

    let init_complete_file = matches.value_of("init_complete_file");

    let rpc_bootstrap_config = RpcBootstrapConfig {
        no_genesis_fetch: matches.is_present("no_genesis_fetch"),
        no_snapshot_fetch: matches.is_present("no_snapshot_fetch"),
        no_check_vote_account: matches.is_present("no_check_vote_account"),
        no_untrusted_rpc: matches.is_present("no_untrusted_rpc"),
        max_genesis_archive_unpacked_size: value_t_or_exit!(
            matches,
            "max_genesis_archive_unpacked_size",
            u64
        ),
    };

    let private_rpc = matches.is_present("private_rpc");
    let no_port_check = matches.is_present("no_port_check");
    let no_rocksdb_compaction = matches.is_present("no_rocksdb_compaction");
    let rocksdb_compaction_interval = value_t!(matches, "rocksdb_compaction_interval", u64).ok();
    let rocksdb_max_compaction_jitter =
        value_t!(matches, "rocksdb_max_compaction_jitter", u64).ok();
    let tpu_coalesce_ms =
        value_t!(matches, "tpu_coalesce_ms", u64).unwrap_or(DEFAULT_TPU_COALESCE_MS);
    let wal_recovery_mode = matches
        .value_of("wal_recovery_mode")
        .map(BlockstoreRecoveryMode::from);

    // Canonicalize ledger path to avoid issues with symlink creation
    let _ = fs::create_dir_all(&ledger_path);
    let ledger_path = fs::canonicalize(&ledger_path).unwrap_or_else(|err| {
        eprintln!("Unable to access ledger path: {:?}", err);
        exit(1);
    });

    let debug_keys: Option<Arc<HashSet<_>>> = if matches.is_present("debug_key") {
        Some(Arc::new(
            values_t_or_exit!(matches, "debug_key", Pubkey)
                .into_iter()
                .collect(),
        ))
    } else {
        None
    };

    let trusted_validators = validators_set(
        &identity_keypair.pubkey(),
        &matches,
        "trusted_validators",
        "--trusted-validator",
    );
    let repair_validators = validators_set(
        &identity_keypair.pubkey(),
        &matches,
        "repair_validators",
        "--repair-validator",
    );
    let gossip_validators = validators_set(
        &identity_keypair.pubkey(),
        &matches,
        "gossip_validators",
        "--gossip-validator",
    );

    let bind_address = solana_net_utils::parse_host(matches.value_of("bind_address").unwrap())
        .expect("invalid bind_address");
    let rpc_bind_address = if matches.is_present("rpc_bind_address") {
        solana_net_utils::parse_host(matches.value_of("rpc_bind_address").unwrap())
            .expect("invalid rpc_bind_address")
    } else {
        bind_address
    };

    let contact_debug_interval = value_t_or_exit!(matches, "contact_debug_interval", u64);

    let account_indexes: HashSet<AccountIndex> = matches
        .values_of("account_indexes")
        .unwrap_or_default()
        .map(|value| match value {
            "program-id" => AccountIndex::ProgramId,
            "spl-token-mint" => AccountIndex::SplTokenMint,
            "spl-token-owner" => AccountIndex::SplTokenOwner,
            _ => unreachable!(),
        })
        .collect();

    let restricted_repair_only_mode = matches.is_present("restricted_repair_only_mode");
    let mut validator_config = ValidatorConfig {
        require_tower: matches.is_present("require_tower"),
        dev_halt_at_slot: value_t!(matches, "dev_halt_at_slot", Slot).ok(),
        cuda: matches.is_present("cuda"),
        expected_genesis_hash: matches
            .value_of("expected_genesis_hash")
            .map(|s| Hash::from_str(&s).unwrap()),
        expected_bank_hash: matches
            .value_of("expected_bank_hash")
            .map(|s| Hash::from_str(&s).unwrap()),
        expected_shred_version: value_t!(matches, "expected_shred_version", u16).ok(),
        new_hard_forks: hardforks_of(&matches, "hard_forks"),
        rpc_config: JsonRpcConfig {
            enable_rpc_transaction_history: matches.is_present("enable_rpc_transaction_history"),
            enable_cpi_and_log_storage: matches.is_present("enable_cpi_and_log_storage"),
            enable_bigtable_ledger_storage: matches
                .is_present("enable_rpc_bigtable_ledger_storage"),
            enable_bigtable_ledger_upload: matches.is_present("enable_bigtable_ledger_upload"),
            identity_pubkey: identity_keypair.pubkey(),
            faucet_addr: matches.value_of("rpc_faucet_addr").map(|address| {
                solana_net_utils::parse_host_port(address).expect("failed to parse faucet address")
            }),
            minimal_api: matches.is_present("minimal_rpc_api"),
            max_multiple_accounts: Some(value_t_or_exit!(
                matches,
                "rpc_max_multiple_accounts",
                usize
            )),
            health_check_slot_distance: value_t_or_exit!(
                matches,
                "health_check_slot_distance",
                u64
            ),
            rpc_threads: value_t_or_exit!(matches, "rpc_threads", usize),
            rpc_bigtable_timeout: value_t!(matches, "rpc_bigtable_timeout", u64)
                .ok()
                .map(Duration::from_secs),
            account_indexes: account_indexes.clone(),
        },
        rpc_addrs: value_t!(matches, "rpc_port", u16).ok().map(|rpc_port| {
            (
                SocketAddr::new(rpc_bind_address, rpc_port),
                SocketAddr::new(rpc_bind_address, rpc_port + 1),
                // If additional ports are added, +2 needs to be skipped to avoid a conflict with
                // the websocket port (which is +2) in web3.js This odd port shifting is tracked at
                // https://github.com/solana-labs/solana/issues/12250
            )
        }),
        pubsub_config: PubSubConfig {
            enable_vote_subscription: matches.is_present("rpc_pubsub_enable_vote_subscription"),
            max_connections: value_t_or_exit!(matches, "rpc_pubsub_max_connections", usize),
            max_fragment_size: value_t_or_exit!(matches, "rpc_pubsub_max_fragment_size", usize),
            max_in_buffer_capacity: value_t_or_exit!(
                matches,
                "rpc_pubsub_max_in_buffer_capacity",
                usize
            ),
            max_out_buffer_capacity: value_t_or_exit!(
                matches,
                "rpc_pubsub_max_out_buffer_capacity",
                usize
            ),
        },
        voting_disabled: matches.is_present("no_voting") || restricted_repair_only_mode,
        wait_for_supermajority: value_t!(matches, "wait_for_supermajority", Slot).ok(),
        trusted_validators,
        repair_validators,
        gossip_validators,
        frozen_accounts: values_t!(matches, "frozen_accounts", Pubkey).unwrap_or_default(),
        no_rocksdb_compaction,
        rocksdb_compaction_interval,
        rocksdb_max_compaction_jitter,
        wal_recovery_mode,
        poh_verify: !matches.is_present("skip_poh_verify"),
        debug_keys,
        contact_debug_interval,
        bpf_jit: !matches.is_present("no_bpf_jit"),
        send_transaction_retry_ms: value_t_or_exit!(matches, "rpc_send_transaction_retry_ms", u64),
        send_transaction_leader_forward_count: value_t_or_exit!(
            matches,
            "rpc_send_transaction_leader_forward_count",
            u64
        ),
        no_poh_speed_test: matches.is_present("no_poh_speed_test"),
        poh_pinned_cpu_core: value_of(&matches, "poh_pinned_cpu_core")
            .unwrap_or(poh_service::DEFAULT_PINNED_CPU_CORE),
        poh_hashes_per_batch: value_of(&matches, "poh_hashes_per_batch")
            .unwrap_or(poh_service::DEFAULT_HASHES_PER_BATCH),
        account_indexes,
        accounts_db_caching_enabled: !matches.is_present("no_accounts_db_caching"),
        accounts_db_test_hash_calculation: matches.is_present("accounts_db_test_hash_calculation"),
        accounts_db_use_index_hash_calculation: matches.is_present("accounts_db_index_hashing"),
        tpu_coalesce_ms,
        ..ValidatorConfig::default()
    };

    let vote_account = pubkey_of(&matches, "vote_account").unwrap_or_else(|| {
        if !validator_config.voting_disabled {
            warn!("--vote-account not specified, validator will not vote");
            validator_config.voting_disabled = true;
        }
        Keypair::new().pubkey()
    });

    let dynamic_port_range =
        solana_net_utils::parse_port_range(matches.value_of("dynamic_port_range").unwrap())
            .expect("invalid dynamic_port_range");

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
    let account_shrink_paths: Option<Vec<PathBuf>> =
        values_t!(matches, "account_shrink_path", String)
            .map(|shrink_paths| shrink_paths.into_iter().map(PathBuf::from).collect())
            .ok();

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

    validator_config.account_shrink_paths = account_shrink_paths.map(|paths| {
        paths
            .into_iter()
            .map(|account_path| {
                match fs::create_dir_all(&account_path)
                    .and_then(|_| fs::canonicalize(&account_path))
                {
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
            .collect()
    });

    let snapshot_interval_slots = value_t_or_exit!(matches, "snapshot_interval_slots", u64);
    let maximum_local_snapshot_age = value_t_or_exit!(matches, "maximum_local_snapshot_age", u64);
    let snapshot_path = ledger_path.join("snapshot");
    fs::create_dir_all(&snapshot_path).unwrap_or_else(|err| {
        eprintln!(
            "Failed to create snapshots directory {:?}: {}",
            snapshot_path, err
        );
        exit(1);
    });

    let archive_format = {
        let archive_format_str = value_t_or_exit!(matches, "snapshot_archive_format", String);
        match archive_format_str.as_str() {
            "bz2" => ArchiveFormat::TarBzip2,
            "gzip" => ArchiveFormat::TarGzip,
            "zstd" => ArchiveFormat::TarZstd,
            "tar" | "none" => ArchiveFormat::Tar,
            _ => panic!("Archive format not recognized: {}", archive_format_str),
        }
    };

    let snapshot_version =
        matches
            .value_of("snapshot_version")
            .map_or(SnapshotVersion::default(), |s| {
                s.parse::<SnapshotVersion>().unwrap_or_else(|err| {
                    eprintln!("Error: {}", err);
                    exit(1)
                })
            });
    validator_config.snapshot_config = Some(SnapshotConfig {
        snapshot_interval_slots: if snapshot_interval_slots > 0 {
            snapshot_interval_slots
        } else {
            std::u64::MAX
        },
        snapshot_path,
        snapshot_package_output_path: ledger_path.clone(),
        archive_format,
        snapshot_version,
    });

    validator_config.accounts_hash_interval_slots =
        value_t_or_exit!(matches, "accounts_hash_interval_slots", u64);
    if validator_config.accounts_hash_interval_slots == 0 {
        eprintln!("Accounts hash interval should not be 0.");
        exit(1);
    }
    if is_snapshot_config_invalid(
        snapshot_interval_slots,
        validator_config.accounts_hash_interval_slots,
    ) {
        eprintln!("Invalid snapshot interval provided ({}), must be a multiple of accounts_hash_interval_slots ({})",
            snapshot_interval_slots,
            validator_config.accounts_hash_interval_slots,
        );
        exit(1);
    }

    if matches.is_present("limit_ledger_size") {
        let limit_ledger_size = match matches.value_of("limit_ledger_size") {
            Some(_) => value_t_or_exit!(matches, "limit_ledger_size", u64),
            None => DEFAULT_MAX_LEDGER_SHREDS,
        };
        if limit_ledger_size < DEFAULT_MIN_MAX_LEDGER_SHREDS {
            eprintln!(
                "The provided --limit-ledger-size value was too small, the minimum value is {}",
                DEFAULT_MIN_MAX_LEDGER_SHREDS
            );
            exit(1);
        }
        validator_config.max_ledger_shreds = Some(limit_ledger_size);
    }

    if matches.is_present("halt_on_trusted_validators_accounts_hash_mismatch") {
        validator_config.halt_on_trusted_validators_accounts_hash_mismatch = true;
    }

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

    let public_rpc_addr = matches.value_of("public_rpc_addr").map(|addr| {
        solana_net_utils::parse_host_port(addr).unwrap_or_else(|e| {
            eprintln!("failed to parse public rpc address: {}", e);
            exit(1);
        })
    });

    let mut ledger_fd_lock = FdLock::new(fs::File::open(&ledger_path).unwrap());
    let _ledger_lock = ledger_fd_lock.try_lock().unwrap_or_else(|_| {
        println!(
            "Error: Unable to lock {} directory. Check if another validator is running",
            ledger_path.display()
        );
        exit(1);
    });

    let logfile = {
        let logfile = matches
            .value_of("logfile")
            .map(|s| s.into())
            .unwrap_or_else(|| format!("solana-validator-{}.log", identity_keypair.pubkey()));

        if logfile == "-" {
            None
        } else {
            println!("log file: {}", logfile);
            Some(logfile)
        }
    };
    let use_progress_bar = logfile.is_none();
    let _logger_thread = redirect_stderr_to_file(logfile);

    info!("{} {}", crate_name!(), solana_version::version!());
    info!("Starting validator with: {:#?}", std::env::args_os());

    let start_progress = Arc::new(RwLock::new(ValidatorStartProgress::default()));
    admin_rpc_service::run(
        &ledger_path,
        admin_rpc_service::AdminRpcRequestMetadata {
            rpc_addr: validator_config.rpc_addrs.map(|(rpc_addr, _)| rpc_addr),
            start_time: std::time::SystemTime::now(),
            validator_exit: validator_config.validator_exit.clone(),
            start_progress: start_progress.clone(),
        },
    );

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

    let cluster_entrypoints = entrypoint_addrs
        .iter()
        .map(ContactInfo::new_gossip_entry_point)
        .collect::<Vec<_>>();

    let mut node = Node::new_with_external_ip(
        &identity_keypair.pubkey(),
        &gossip_addr,
        dynamic_port_range,
        bind_address,
    );

    if restricted_repair_only_mode {
        let any = SocketAddr::new(std::net::IpAddr::V4(std::net::Ipv4Addr::new(0, 0, 0, 0)), 0);
        // When in --restricted_repair_only_mode is enabled only the gossip and repair ports
        // need to be reachable by the entrypoint to respond to gossip pull requests and repair
        // requests initiated by the node.  All other ports are unused.
        node.info.tpu = any;
        node.info.tpu_forwards = any;
        node.info.tvu = any;
        node.info.tvu_forwards = any;
        node.info.serve_repair = any;

        // A node in this configuration shouldn't be an entrypoint to other nodes
        node.sockets.ip_echo = None;
    }

    if !private_rpc {
        if let Some(public_rpc_addr) = public_rpc_addr {
            node.info.rpc = public_rpc_addr;
            node.info.rpc_pubsub = public_rpc_addr;
        } else if let Some((rpc_addr, rpc_pubsub_addr)) = validator_config.rpc_addrs {
            node.info.rpc = SocketAddr::new(node.info.gossip.ip(), rpc_addr.port());
            node.info.rpc_pubsub = SocketAddr::new(node.info.gossip.ip(), rpc_pubsub_addr.port());
        }
    }

    solana_metrics::set_host_id(identity_keypair.pubkey().to_string());
    solana_metrics::set_panic_hook("validator");

    if validator_config.cuda {
        solana_perf::perf_libs::init_cuda();
        enable_recycler_warming();
    }
    solana_ledger::entry::init_poh();
    solana_runtime::snapshot_utils::remove_tmp_snapshot_archives(&ledger_path);

    let should_check_duplicate_instance = !matches.is_present("no_duplicate_instance_check");
    if !cluster_entrypoints.is_empty() {
        rpc_bootstrap(
            &node,
            &identity_keypair,
            &ledger_path,
            &vote_account,
            &authorized_voter_keypairs,
            &cluster_entrypoints,
            &mut validator_config,
            rpc_bootstrap_config,
            no_port_check,
            use_progress_bar,
            maximum_local_snapshot_age,
            should_check_duplicate_instance,
            &start_progress,
        );
        *start_progress.write().unwrap() = ValidatorStartProgress::Initializing;
    }

    if operation == Operation::Initialize {
        info!("Validator ledger initialization complete");
        return;
    }
    let validator = Validator::new(
        node,
        &identity_keypair,
        &ledger_path,
        &vote_account,
        authorized_voter_keypairs,
        cluster_entrypoints,
        &validator_config,
        should_check_duplicate_instance,
        start_progress,
    );

    if let Some(filename) = init_complete_file {
        File::create(filename).unwrap_or_else(|_| {
            error!("Unable to create: {}", filename);
            exit(1);
        });
    }
    info!("Validator initialized");
    validator.join();
    info!("Validator exiting..");
}
