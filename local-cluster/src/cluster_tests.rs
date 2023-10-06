/// Cluster independent integration tests
///
/// All tests must start from an entry point and a funding keypair and
/// discover the rest of the network.
use log::*;
use {
    rand::{thread_rng, Rng},
    rayon::prelude::*,
    solana_client::{
        connection_cache::{ConnectionCache, Protocol},
        thin_client::ThinClient,
    },
    solana_core::consensus::VOTE_THRESHOLD_DEPTH,
    solana_entry::entry::{Entry, EntrySlice},
    solana_gossip::{
        cluster_info::{self, ClusterInfo},
        contact_info::{ContactInfo, LegacyContactInfo},
        crds::Cursor,
        crds_value::{self, CrdsData, CrdsValue, CrdsValueLabel},
        gossip_error::GossipError,
        gossip_service::{self, discover_cluster, GossipService},
    },
    solana_ledger::blockstore::Blockstore,
    solana_sdk::{
        client::SyncClient,
        clock::{self, Slot, NUM_CONSECUTIVE_LEADER_SLOTS},
        commitment_config::CommitmentConfig,
        epoch_schedule::MINIMUM_SLOTS_PER_EPOCH,
        exit::Exit,
        hash::Hash,
        poh_config::PohConfig,
        pubkey::Pubkey,
        signature::{Keypair, Signature, Signer},
        system_transaction,
        timing::{duration_as_ms, timestamp},
        transaction::Transaction,
        transport::TransportError,
    },
    solana_streamer::socket::SocketAddrSpace,
    solana_vote::vote_transaction::VoteTransaction,
    solana_vote_program::vote_transaction,
    std::{
        borrow::Borrow,
        collections::{HashMap, HashSet},
        net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener},
        path::Path,
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc, RwLock,
        },
        thread::{sleep, JoinHandle},
        time::{Duration, Instant},
    },
};

pub fn get_client_facing_addr<T: Borrow<LegacyContactInfo>>(
    protocol: Protocol,
    contact_info: T,
) -> (SocketAddr, SocketAddr) {
    let contact_info = contact_info.borrow();
    let rpc = contact_info.rpc().unwrap();
    let mut tpu = contact_info.tpu(protocol).unwrap();
    // QUIC certificate authentication requires the IP Address to match. ContactInfo might have
    // 0.0.0.0 as the IP instead of 127.0.0.1.
    tpu.set_ip(IpAddr::V4(Ipv4Addr::LOCALHOST));
    (rpc, tpu)
}

/// Spend and verify from every node in the network
pub fn spend_and_verify_all_nodes<S: ::std::hash::BuildHasher + Sync + Send>(
    entry_point_info: &ContactInfo,
    funding_keypair: &Keypair,
    nodes: usize,
    ignore_nodes: HashSet<Pubkey, S>,
    socket_addr_space: SocketAddrSpace,
    connection_cache: &Arc<ConnectionCache>,
) {
    let cluster_nodes = discover_cluster(
        &entry_point_info.gossip().unwrap(),
        nodes,
        socket_addr_space,
    )
    .unwrap();
    assert!(cluster_nodes.len() >= nodes);
    let ignore_nodes = Arc::new(ignore_nodes);
    cluster_nodes.par_iter().for_each(|ingress_node| {
        if ignore_nodes.contains(ingress_node.pubkey()) {
            return;
        }
        let random_keypair = Keypair::new();
        let (rpc, tpu) = get_client_facing_addr(connection_cache.protocol(), ingress_node);
        let client = ThinClient::new(rpc, tpu, connection_cache.clone());
        let bal = client
            .poll_get_balance_with_commitment(
                &funding_keypair.pubkey(),
                CommitmentConfig::processed(),
            )
            .expect("balance in source");
        assert!(bal > 0);
        let (blockhash, _) = client
            .get_latest_blockhash_with_commitment(CommitmentConfig::confirmed())
            .unwrap();
        let mut transaction =
            system_transaction::transfer(funding_keypair, &random_keypair.pubkey(), 1, blockhash);
        let confs = VOTE_THRESHOLD_DEPTH + 1;
        let sig = client
            .retry_transfer_until_confirmed(funding_keypair, &mut transaction, 10, confs)
            .unwrap();
        for validator in &cluster_nodes {
            if ignore_nodes.contains(validator.pubkey()) {
                continue;
            }
            let (rpc, tpu) = get_client_facing_addr(connection_cache.protocol(), validator);
            let client = ThinClient::new(rpc, tpu, connection_cache.clone());
            client.poll_for_signature_confirmation(&sig, confs).unwrap();
        }
    });
}

pub fn verify_balances<S: ::std::hash::BuildHasher>(
    expected_balances: HashMap<Pubkey, u64, S>,
    node: &ContactInfo,
    connection_cache: Arc<ConnectionCache>,
) {
    let (rpc, tpu) = LegacyContactInfo::try_from(node)
        .map(|node| get_client_facing_addr(connection_cache.protocol(), node))
        .unwrap();
    let client = ThinClient::new(rpc, tpu, connection_cache);
    for (pk, b) in expected_balances {
        let bal = client
            .poll_get_balance_with_commitment(&pk, CommitmentConfig::processed())
            .expect("balance in source");
        assert_eq!(bal, b);
    }
}

pub fn send_many_transactions(
    node: &LegacyContactInfo,
    funding_keypair: &Keypair,
    connection_cache: &Arc<ConnectionCache>,
    max_tokens_per_transfer: u64,
    num_txs: u64,
) -> HashMap<Pubkey, u64> {
    let (rpc, tpu) = get_client_facing_addr(connection_cache.protocol(), node);
    let client = ThinClient::new(rpc, tpu, connection_cache.clone());
    let mut expected_balances = HashMap::new();
    for _ in 0..num_txs {
        let random_keypair = Keypair::new();
        let bal = client
            .poll_get_balance_with_commitment(
                &funding_keypair.pubkey(),
                CommitmentConfig::processed(),
            )
            .expect("balance in source");
        assert!(bal > 0);
        let (blockhash, _) = client
            .get_latest_blockhash_with_commitment(CommitmentConfig::processed())
            .unwrap();
        let transfer_amount = thread_rng().gen_range(1..max_tokens_per_transfer);

        let mut transaction = system_transaction::transfer(
            funding_keypair,
            &random_keypair.pubkey(),
            transfer_amount,
            blockhash,
        );

        client
            .retry_transfer(funding_keypair, &mut transaction, 5)
            .unwrap();

        expected_balances.insert(random_keypair.pubkey(), transfer_amount);
    }

    expected_balances
}

pub fn verify_ledger_ticks(ledger_path: &Path, ticks_per_slot: usize) {
    let ledger = Blockstore::open(ledger_path).unwrap();
    let zeroth_slot = ledger.get_slot_entries(0, 0).unwrap();
    let last_id = zeroth_slot.last().unwrap().hash;
    let next_slots = ledger.get_slots_since(&[0]).unwrap().remove(&0).unwrap();
    let mut pending_slots: Vec<_> = next_slots
        .into_iter()
        .map(|slot| (slot, 0, last_id))
        .collect();
    while let Some((slot, parent_slot, last_id)) = pending_slots.pop() {
        let next_slots = ledger
            .get_slots_since(&[slot])
            .unwrap()
            .remove(&slot)
            .unwrap();

        // If you're not the last slot, you should have a full set of ticks
        let should_verify_ticks = if !next_slots.is_empty() {
            Some((slot - parent_slot) as usize * ticks_per_slot)
        } else {
            None
        };

        let last_id = verify_slot_ticks(&ledger, slot, &last_id, should_verify_ticks);
        pending_slots.extend(
            next_slots
                .into_iter()
                .map(|child_slot| (child_slot, slot, last_id)),
        );
    }
}

pub fn sleep_n_epochs(
    num_epochs: f64,
    config: &PohConfig,
    ticks_per_slot: u64,
    slots_per_epoch: u64,
) {
    let num_ticks_per_second = (1000 / duration_as_ms(&config.target_tick_duration)) as f64;
    let num_ticks_to_sleep = num_epochs * ticks_per_slot as f64 * slots_per_epoch as f64;
    let secs = ((num_ticks_to_sleep + num_ticks_per_second - 1.0) / num_ticks_per_second) as u64;
    warn!("sleep_n_epochs: {} seconds", secs);
    sleep(Duration::from_secs(secs));
}

pub fn kill_entry_and_spend_and_verify_rest(
    entry_point_info: &ContactInfo,
    entry_point_validator_exit: &Arc<RwLock<Exit>>,
    funding_keypair: &Keypair,
    connection_cache: &Arc<ConnectionCache>,
    nodes: usize,
    slot_millis: u64,
    socket_addr_space: SocketAddrSpace,
) {
    info!("kill_entry_and_spend_and_verify_rest...");
    let cluster_nodes = discover_cluster(
        &entry_point_info.gossip().unwrap(),
        nodes,
        socket_addr_space,
    )
    .unwrap();
    assert!(cluster_nodes.len() >= nodes);
    let (rpc, tpu) = LegacyContactInfo::try_from(entry_point_info)
        .map(|node| get_client_facing_addr(connection_cache.protocol(), node))
        .unwrap();
    let client = ThinClient::new(rpc, tpu, connection_cache.clone());

    // sleep long enough to make sure we are in epoch 3
    let first_two_epoch_slots = MINIMUM_SLOTS_PER_EPOCH * (3 + 1);

    for ingress_node in &cluster_nodes {
        client
            .poll_get_balance_with_commitment(ingress_node.pubkey(), CommitmentConfig::processed())
            .unwrap_or_else(|err| panic!("Node {} has no balance: {}", ingress_node.pubkey(), err));
    }

    info!("sleeping for 2 leader fortnights");
    sleep(Duration::from_millis(slot_millis * first_two_epoch_slots));
    info!("done sleeping for first 2 warmup epochs");
    info!("killing entry point: {}", entry_point_info.pubkey());
    entry_point_validator_exit.write().unwrap().exit();
    info!("sleeping for some time");
    sleep(Duration::from_millis(
        slot_millis * NUM_CONSECUTIVE_LEADER_SLOTS,
    ));
    info!("done sleeping for 2 fortnights");
    for ingress_node in &cluster_nodes {
        if ingress_node.pubkey() == entry_point_info.pubkey() {
            info!("ingress_node.id == entry_point_info.id, continuing...");
            continue;
        }

        let (rpc, tpu) = get_client_facing_addr(connection_cache.protocol(), ingress_node);
        let client = ThinClient::new(rpc, tpu, connection_cache.clone());
        let balance = client
            .poll_get_balance_with_commitment(
                &funding_keypair.pubkey(),
                CommitmentConfig::processed(),
            )
            .expect("balance in source");
        assert_ne!(balance, 0);

        let mut result = Ok(());
        let mut retries = 0;
        loop {
            retries += 1;
            if retries > 5 {
                result.unwrap();
            }

            let random_keypair = Keypair::new();
            let (blockhash, _) = client
                .get_latest_blockhash_with_commitment(CommitmentConfig::processed())
                .unwrap();
            let mut transaction = system_transaction::transfer(
                funding_keypair,
                &random_keypair.pubkey(),
                1,
                blockhash,
            );

            let confs = VOTE_THRESHOLD_DEPTH + 1;
            let sig = {
                let sig = client.retry_transfer_until_confirmed(
                    funding_keypair,
                    &mut transaction,
                    5,
                    confs,
                );
                match sig {
                    Err(e) => {
                        result = Err(e);
                        continue;
                    }

                    Ok(sig) => sig,
                }
            };
            info!("poll_all_nodes_for_signature()");
            match poll_all_nodes_for_signature(
                entry_point_info,
                &cluster_nodes,
                connection_cache,
                &sig,
                confs,
            ) {
                Err(e) => {
                    info!("poll_all_nodes_for_signature() failed {:?}", e);
                    result = Err(e);
                }
                Ok(()) => {
                    info!("poll_all_nodes_for_signature() succeeded, done.");
                    break;
                }
            }
        }
    }
}

pub fn check_for_new_roots(
    num_new_roots: usize,
    contact_infos: &[ContactInfo],
    connection_cache: &Arc<ConnectionCache>,
    test_name: &str,
) {
    let mut roots = vec![HashSet::new(); contact_infos.len()];
    let mut done = false;
    let mut last_print = Instant::now();
    let loop_start = Instant::now();
    let loop_timeout = Duration::from_secs(180);
    let mut num_roots_map = HashMap::new();
    while !done {
        assert!(loop_start.elapsed() < loop_timeout);

        for (i, ingress_node) in contact_infos.iter().enumerate() {
            let (rpc, tpu) = LegacyContactInfo::try_from(ingress_node)
                .map(|node| get_client_facing_addr(connection_cache.protocol(), node))
                .unwrap();
            let client = ThinClient::new(rpc, tpu, connection_cache.clone());
            let root_slot = client
                .get_slot_with_commitment(CommitmentConfig::finalized())
                .unwrap_or(0);
            roots[i].insert(root_slot);
            num_roots_map.insert(*ingress_node.pubkey(), roots[i].len());
            let num_roots = roots.iter().map(|r| r.len()).min().unwrap();
            done = num_roots >= num_new_roots;
            if done || last_print.elapsed().as_secs() > 3 {
                info!(
                    "{} waiting for {} new roots.. observed: {:?}",
                    test_name, num_new_roots, num_roots_map
                );
                last_print = Instant::now();
            }
        }
        sleep(Duration::from_millis(clock::DEFAULT_MS_PER_SLOT / 2));
    }
}

pub fn check_no_new_roots(
    num_slots_to_wait: usize,
    contact_infos: &[LegacyContactInfo],
    connection_cache: &Arc<ConnectionCache>,
    test_name: &str,
) {
    assert!(!contact_infos.is_empty());
    let mut roots = vec![0; contact_infos.len()];
    let max_slot = contact_infos
        .iter()
        .enumerate()
        .map(|(i, ingress_node)| {
            let (rpc, tpu) = get_client_facing_addr(connection_cache.protocol(), ingress_node);
            let client = ThinClient::new(rpc, tpu, connection_cache.clone());
            let initial_root = client
                .get_slot()
                .unwrap_or_else(|_| panic!("get_slot for {} failed", ingress_node.pubkey()));
            roots[i] = initial_root;
            client
                .get_slot_with_commitment(CommitmentConfig::processed())
                .unwrap_or_else(|_| panic!("get_slot for {} failed", ingress_node.pubkey()))
        })
        .max()
        .unwrap();

    let end_slot = max_slot + num_slots_to_wait as u64;
    let mut current_slot;
    let mut last_print = Instant::now();
    let mut reached_end_slot = false;
    loop {
        for contact_info in contact_infos {
            let (rpc, tpu) = get_client_facing_addr(connection_cache.protocol(), contact_info);
            let client = ThinClient::new(rpc, tpu, connection_cache.clone());
            current_slot = client
                .get_slot_with_commitment(CommitmentConfig::processed())
                .unwrap_or_else(|_| panic!("get_slot for {} failed", contact_infos[0].pubkey()));
            if current_slot > end_slot {
                reached_end_slot = true;
                break;
            }
            if last_print.elapsed().as_secs() > 3 {
                info!(
                    "{} current slot: {} on validator: {}, waiting for any validator with slot: {}",
                    test_name,
                    current_slot,
                    contact_info.pubkey(),
                    end_slot
                );
                last_print = Instant::now();
            }
        }
        if reached_end_slot {
            break;
        }
    }

    for (i, ingress_node) in contact_infos.iter().enumerate() {
        let (rpc, tpu) = get_client_facing_addr(connection_cache.protocol(), ingress_node);
        let client = ThinClient::new(rpc, tpu, connection_cache.clone());
        assert_eq!(
            client
                .get_slot()
                .unwrap_or_else(|_| panic!("get_slot for {} failed", ingress_node.pubkey())),
            roots[i]
        );
    }
}

fn poll_all_nodes_for_signature(
    entry_point_info: &ContactInfo,
    cluster_nodes: &[LegacyContactInfo],
    connection_cache: &Arc<ConnectionCache>,
    sig: &Signature,
    confs: usize,
) -> Result<(), TransportError> {
    for validator in cluster_nodes {
        if validator.pubkey() == entry_point_info.pubkey() {
            continue;
        }
        let (rpc, tpu) = get_client_facing_addr(connection_cache.protocol(), validator);
        let client = ThinClient::new(rpc, tpu, connection_cache.clone());
        client.poll_for_signature_confirmation(sig, confs)?;
    }

    Ok(())
}

pub struct GossipVoter {
    pub gossip_service: GossipService,
    pub tcp_listener: Option<TcpListener>,
    pub cluster_info: Arc<ClusterInfo>,
    pub t_voter: JoinHandle<()>,
    pub exit: Arc<AtomicBool>,
}

impl GossipVoter {
    pub fn close(self) {
        self.exit.store(true, Ordering::Relaxed);
        self.t_voter.join().unwrap();
        self.gossip_service.join().unwrap();
    }
}

/// Reads votes from gossip and runs them through `vote_filter` to filter votes that then
/// get passed to `generate_vote_tx` to create votes that are then pushed into gossip as if
/// sent by a node with identity `node_keypair`.
pub fn start_gossip_voter(
    gossip_addr: &SocketAddr,
    node_keypair: &Keypair,
    vote_filter: impl Fn((CrdsValueLabel, Transaction)) -> Option<(VoteTransaction, Transaction)>
        + std::marker::Send
        + 'static,
    mut process_vote_tx: impl FnMut(Slot, &Transaction, &VoteTransaction, &ClusterInfo)
        + std::marker::Send
        + 'static,
    sleep_ms: u64,
) -> GossipVoter {
    let exit = Arc::new(AtomicBool::new(false));
    let (gossip_service, tcp_listener, cluster_info) = gossip_service::make_gossip_node(
        // Need to use our validator's keypair to gossip EpochSlots and votes for our
        // node later.
        node_keypair.insecure_clone(),
        Some(gossip_addr),
        exit.clone(),
        None,
        0,
        false,
        SocketAddrSpace::Unspecified,
    );

    let t_voter = {
        let exit = exit.clone();
        let cluster_info = cluster_info.clone();
        std::thread::spawn(move || {
            let mut cursor = Cursor::default();
            loop {
                if exit.load(Ordering::Relaxed) {
                    return;
                }

                let (labels, votes) = cluster_info.get_votes_with_labels(&mut cursor);
                let mut parsed_vote_iter: Vec<_> = labels
                    .into_iter()
                    .zip(votes)
                    .filter_map(&vote_filter)
                    .collect();

                parsed_vote_iter.sort_by(|(vote, _), (vote2, _)| {
                    vote.last_voted_slot()
                        .unwrap()
                        .cmp(&vote2.last_voted_slot().unwrap())
                });

                for (parsed_vote, leader_vote_tx) in &parsed_vote_iter {
                    if let Some(latest_vote_slot) = parsed_vote.last_voted_slot() {
                        info!("received vote for {}", latest_vote_slot);
                        process_vote_tx(
                            latest_vote_slot,
                            leader_vote_tx,
                            parsed_vote,
                            &cluster_info,
                        )
                    }
                    // Give vote some time to propagate
                    sleep(Duration::from_millis(sleep_ms));
                }

                if parsed_vote_iter.is_empty() {
                    sleep(Duration::from_millis(sleep_ms));
                }
            }
        })
    };

    GossipVoter {
        gossip_service,
        tcp_listener,
        cluster_info,
        t_voter,
        exit,
    }
}

fn get_and_verify_slot_entries(
    blockstore: &Blockstore,
    slot: Slot,
    last_entry: &Hash,
) -> Vec<Entry> {
    let entries = blockstore.get_slot_entries(slot, 0).unwrap();
    assert!(entries.verify(last_entry));
    entries
}

fn verify_slot_ticks(
    blockstore: &Blockstore,
    slot: Slot,
    last_entry: &Hash,
    expected_num_ticks: Option<usize>,
) -> Hash {
    let entries = get_and_verify_slot_entries(blockstore, slot, last_entry);
    let num_ticks: usize = entries.iter().map(|entry| entry.is_tick() as usize).sum();
    if let Some(expected_num_ticks) = expected_num_ticks {
        assert_eq!(num_ticks, expected_num_ticks);
    }
    entries.last().unwrap().hash
}

pub fn submit_vote_to_cluster_gossip(
    node_keypair: &Keypair,
    vote_keypair: &Keypair,
    vote_slot: Slot,
    vote_hash: Hash,
    blockhash: Hash,
    gossip_addr: SocketAddr,
    socket_addr_space: &SocketAddrSpace,
) -> Result<(), GossipError> {
    let vote_tx = vote_transaction::new_vote_transaction(
        vec![vote_slot],
        vote_hash,
        blockhash,
        node_keypair,
        vote_keypair,
        vote_keypair,
        None,
    );

    cluster_info::push_messages_to_peer(
        vec![CrdsValue::new_signed(
            CrdsData::Vote(
                0,
                crds_value::Vote::new(node_keypair.pubkey(), vote_tx, timestamp()).unwrap(),
            ),
            node_keypair,
        )],
        node_keypair.pubkey(),
        gossip_addr,
        socket_addr_space,
    )
}
