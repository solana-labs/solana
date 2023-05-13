//! If a test takes over 100s to run on CI, move it here so that it's clear where the
//! biggest improvements to CI times can be found.
#![allow(clippy::integer_arithmetic)]
use {
    common::*,
    log::*,
    rand::{thread_rng, Rng},
    serial_test::serial,
    solana_core::validator::ValidatorConfig,
    solana_gossip::gossip_service::discover_cluster,
    solana_ledger::{
        ancestor_iterator::AncestorIterator, blockstore::Blockstore, leader_schedule::FixedSchedule,
    },
    solana_local_cluster::{
        cluster::Cluster,
        cluster_tests,
        local_cluster::{ClusterConfig, LocalCluster},
        validator_configs::*,
    },
    solana_sdk::{
        client::SyncClient,
        clock::Slot,
        hash::{extend_and_hash, Hash},
        poh_config::PohConfig,
        signature::{Keypair, Signer},
    },
    solana_streamer::socket::SocketAddrSpace,
    std::{
        collections::HashSet,
        sync::Arc,
        thread::sleep,
        time::{Duration, Instant},
    },
    trees::tr,
};

mod common;

#[test]
#[serial]
fn test_cluster_partition_1_1() {
    let empty = |_: &mut LocalCluster, _: &mut ()| {};
    let on_partition_resolved = |cluster: &mut LocalCluster, _: &mut ()| {
        cluster.check_for_new_roots(16, "PARTITION_TEST", SocketAddrSpace::Unspecified);
    };
    run_cluster_partition(
        &[1, 1],
        None,
        (),
        empty,
        empty,
        on_partition_resolved,
        None,
        vec![],
    )
}

#[test]
#[serial]
fn test_cluster_partition_1_1_1() {
    let empty = |_: &mut LocalCluster, _: &mut ()| {};
    let on_partition_resolved = |cluster: &mut LocalCluster, _: &mut ()| {
        cluster.check_for_new_roots(16, "PARTITION_TEST", SocketAddrSpace::Unspecified);
    };
    run_cluster_partition(
        &[1, 1, 1],
        None,
        (),
        empty,
        empty,
        on_partition_resolved,
        None,
        vec![],
    )
}

#[test]
#[serial]
fn test_consistency_halt() {
    solana_logger::setup_with_default(RUST_LOG_FILTER);
    let snapshot_interval_slots = 20;
    let num_account_paths = 1;

    // Create cluster with a leader producing bad snapshot hashes.
    let mut leader_snapshot_test_config =
        setup_snapshot_validator_config(snapshot_interval_slots, num_account_paths);

    // Prepare fault hash injection for testing.
    leader_snapshot_test_config
        .validator_config
        .accounts_hash_fault_injector = Some(|hash: &Hash, slot: Slot| {
        const FAULT_INJECTION_RATE_SLOTS: u64 = 40; // Inject a fault hash every 40 slots
        (slot % FAULT_INJECTION_RATE_SLOTS == 0).then(|| {
            let rand = thread_rng().gen_range(0, 10);
            let fault_hash = extend_and_hash(hash, &[rand]);
            warn!("inserting fault at slot: {}", slot);
            fault_hash
        })
    });

    let validator_stake = DEFAULT_NODE_STAKE;
    let mut config = ClusterConfig {
        node_stakes: vec![validator_stake],
        cluster_lamports: DEFAULT_CLUSTER_LAMPORTS,
        validator_configs: vec![leader_snapshot_test_config.validator_config],
        ..ClusterConfig::default()
    };

    let mut cluster = LocalCluster::new(&mut config, SocketAddrSpace::Unspecified);

    sleep(Duration::from_millis(5000));
    let cluster_nodes = discover_cluster(
        &cluster.entry_point_info.gossip().unwrap(),
        1,
        SocketAddrSpace::Unspecified,
    )
    .unwrap();
    info!("num_nodes: {}", cluster_nodes.len());

    // Add a validator with the leader as trusted, it should halt when it detects
    // mismatch.
    let mut validator_snapshot_test_config =
        setup_snapshot_validator_config(snapshot_interval_slots, num_account_paths);

    let mut known_validators = HashSet::new();
    known_validators.insert(*cluster_nodes[0].pubkey());

    validator_snapshot_test_config
        .validator_config
        .known_validators = Some(known_validators);
    validator_snapshot_test_config
        .validator_config
        .halt_on_known_validators_accounts_hash_mismatch = true;

    warn!("adding a validator");
    cluster.add_validator(
        &validator_snapshot_test_config.validator_config,
        validator_stake,
        Arc::new(Keypair::new()),
        None,
        SocketAddrSpace::Unspecified,
    );
    let num_nodes = 2;
    assert_eq!(
        discover_cluster(
            &cluster.entry_point_info.gossip().unwrap(),
            num_nodes,
            SocketAddrSpace::Unspecified
        )
        .unwrap()
        .len(),
        num_nodes
    );

    // Check for only 1 node on the network.
    let mut encountered_error = false;
    loop {
        let discover = discover_cluster(
            &cluster.entry_point_info.gossip().unwrap(),
            2,
            SocketAddrSpace::Unspecified,
        );
        match discover {
            Err(_) => {
                encountered_error = true;
                break;
            }
            Ok(nodes) => {
                if nodes.len() < 2 {
                    encountered_error = true;
                    break;
                }
                info!("checking cluster for fewer nodes.. {:?}", nodes.len());
            }
        }
        let client = cluster
            .get_validator_client(cluster.entry_point_info.pubkey())
            .unwrap();
        if let Ok(slot) = client.get_slot() {
            if slot > 210 {
                break;
            }
            info!("slot: {}", slot);
        }
        sleep(Duration::from_millis(1000));
    }
    assert!(encountered_error);
}

// Cluster needs a supermajority to remain, so the minimum size for this test is 4
#[test]
#[serial]
fn test_leader_failure_4() {
    solana_logger::setup_with_default(RUST_LOG_FILTER);
    error!("test_leader_failure_4");
    let num_nodes = 4;
    let validator_config = ValidatorConfig::default_for_test();
    let mut config = ClusterConfig {
        cluster_lamports: DEFAULT_CLUSTER_LAMPORTS,
        node_stakes: vec![DEFAULT_NODE_STAKE; 4],
        validator_configs: make_identical_validator_configs(&validator_config, num_nodes),
        ..ClusterConfig::default()
    };
    let local = LocalCluster::new(&mut config, SocketAddrSpace::Unspecified);

    cluster_tests::kill_entry_and_spend_and_verify_rest(
        &local.entry_point_info,
        &local
            .validators
            .get(local.entry_point_info.pubkey())
            .unwrap()
            .config
            .validator_exit,
        &local.funding_keypair,
        &local.connection_cache,
        num_nodes,
        config.ticks_per_slot * config.poh_config.target_tick_duration.as_millis() as u64,
        SocketAddrSpace::Unspecified,
    );
}

#[test]
#[serial]
fn test_ledger_cleanup_service() {
    solana_logger::setup_with_default(RUST_LOG_FILTER);
    error!("test_ledger_cleanup_service");
    let num_nodes = 3;
    let validator_config = ValidatorConfig {
        max_ledger_shreds: Some(100),
        ..ValidatorConfig::default_for_test()
    };
    let mut config = ClusterConfig {
        cluster_lamports: DEFAULT_CLUSTER_LAMPORTS,
        poh_config: PohConfig::new_sleep(Duration::from_millis(50)),
        node_stakes: vec![DEFAULT_NODE_STAKE; num_nodes],
        validator_configs: make_identical_validator_configs(&validator_config, num_nodes),
        ..ClusterConfig::default()
    };
    let mut cluster = LocalCluster::new(&mut config, SocketAddrSpace::Unspecified);
    // 200ms/per * 100 = 20 seconds, so sleep a little longer than that.
    sleep(Duration::from_secs(60));

    cluster_tests::spend_and_verify_all_nodes(
        &cluster.entry_point_info,
        &cluster.funding_keypair,
        num_nodes,
        HashSet::new(),
        SocketAddrSpace::Unspecified,
        &cluster.connection_cache,
    );
    cluster.close_preserve_ledgers();
    //check everyone's ledgers and make sure only ~100 slots are stored
    for info in cluster.validators.values() {
        let mut slots = 0;
        let blockstore = Blockstore::open(&info.info.ledger_path).unwrap();
        blockstore
            .slot_meta_iterator(0)
            .unwrap()
            .for_each(|_| slots += 1);
        // with 3 nodes up to 3 slots can be in progress and not complete so max slots in blockstore should be up to 103
        assert!(slots <= 103, "got {slots}");
    }
}

// This test verifies that even if votes from a validator end up taking too long to land, and thus
// some of the referenced slots are slots are no longer present in the slot hashes sysvar,
// consensus can still be attained.
//
// Validator A (60%)
// Validator B (40%)
//                                  / --- 10 --- [..] --- 16 (B is voting, due to network issues is initally not able to see the other fork at all)
//                                 /
// 1 - 2 - 3 - 4 - 5 - 6 - 7 - 8 - 9 (A votes 1 - 9 votes are landing normally. B does the same however votes are not landing)
//                                 \
//                                  \--[..]-- 73  (majority fork)
// A is voting on the majority fork and B wants to switch to this fork however in this majority fork
// the earlier votes for B (1 - 9) never landed so when B eventually goes to vote on 73, slots in
// its local vote state are no longer present in slot hashes.
//
// 1. Wait for B's tower to see local vote state was updated to new fork
// 2. Wait X blocks, check B's vote state on chain has been properly updated
//
// NOTE: it is not reliable for B to organically have 1 to reach 2^16 lockout, so we simulate the 6
// consecutive votes on the minor fork by manually incrementing the confirmation levels for the
// common ancestor votes in tower.
// To allow this test to run in a reasonable time we change the
// slot_hash expiry to 64 slots.

#[test]
fn test_slot_hash_expiry() {
    solana_logger::setup_with_default(RUST_LOG_FILTER);
    solana_sdk::slot_hashes::set_entries_for_tests_only(64);

    let slots_per_epoch = 2048;
    let node_stakes = vec![60 * DEFAULT_NODE_STAKE, 40 * DEFAULT_NODE_STAKE];
    let validator_keys = vec![
        "28bN3xyvrP4E8LwEgtLjhnkb7cY4amQb6DrYAbAYjgRV4GAGgkVM2K7wnxnAS7WDneuavza7x21MiafLu1HkwQt4",
        "2saHBBoTkLMmttmPQP8KfBkcCw45S5cwtV3wTdGCscRC8uxdgvHxpHiWXKx4LvJjNJtnNcbSv5NdheokFFqnNDt8",
    ]
    .iter()
    .map(|s| (Arc::new(Keypair::from_base58_string(s)), true))
    .collect::<Vec<_>>();
    let node_vote_keys = vec![
        "3NDQ3ud86RTVg8hTy2dDWnS4P8NfjhZ2gDgQAJbr3heaKaUVS1FW3sTLKA1GmDrY9aySzsa4QxpDkbLv47yHxzr3",
        "46ZHpHE6PEvXYPu3hf9iQqjBk2ZNDaJ9ejqKWHEjxaQjpAGasKaWKbKHbP3646oZhfgDRzx95DH9PCBKKsoCVngk",
    ]
    .iter()
    .map(|s| Arc::new(Keypair::from_base58_string(s)))
    .collect::<Vec<_>>();
    let vs = validator_keys
        .iter()
        .map(|(kp, _)| kp.pubkey())
        .collect::<Vec<_>>();
    let (a_pubkey, b_pubkey) = (vs[0], vs[1]);

    // We want B to not vote (we are trying to simulate its votes not landing until it gets to the
    // minority fork)
    let mut validator_configs =
        make_identical_validator_configs(&ValidatorConfig::default_for_test(), node_stakes.len());
    validator_configs[1].voting_disabled = true;

    let mut config = ClusterConfig {
        cluster_lamports: DEFAULT_CLUSTER_LAMPORTS + node_stakes.iter().sum::<u64>(),
        node_stakes,
        validator_configs,
        validator_keys: Some(validator_keys),
        node_vote_keys: Some(node_vote_keys),
        slots_per_epoch,
        stakers_slot_offset: slots_per_epoch,
        skip_warmup_slots: true,
        ..ClusterConfig::default()
    };
    let mut cluster = LocalCluster::new(&mut config, SocketAddrSpace::Unspecified);

    let mut common_ancestor_slot = 8;

    let a_ledger_path = cluster.ledger_path(&a_pubkey);
    let b_ledger_path = cluster.ledger_path(&b_pubkey);

    // Immediately kill B (we just needed it for the initial stake distribution)
    info!("Killing B");
    let mut b_info = cluster.exit_node(&b_pubkey);

    // Let A run for a while until we get to the common ancestor
    info!("Letting A run until common_ancestor_slot");
    loop {
        if let Some((last_vote, _)) = last_vote_in_tower(&a_ledger_path, &a_pubkey) {
            if last_vote >= common_ancestor_slot {
                break;
            }
        }
        sleep(Duration::from_millis(100));
    }

    // Keep A running, but setup B so that it thinks it has voted up until common ancestor (but
    // doesn't know anything past that)
    {
        info!("Copying A's ledger to B");
        std::fs::remove_dir_all(&b_info.info.ledger_path).unwrap();
        let mut opt = fs_extra::dir::CopyOptions::new();
        opt.copy_inside = true;
        fs_extra::dir::copy(&a_ledger_path, &b_ledger_path, &opt).unwrap();

        // remove A's tower in B's new copied ledger
        info!("Removing A's tower in B's ledger dir");
        remove_tower(&b_ledger_path, &a_pubkey);

        // load A's tower and save it as B's tower
        info!("Loading A's tower");
        if let Some(mut a_tower) = restore_tower(&a_ledger_path, &a_pubkey) {
            a_tower.node_pubkey = b_pubkey;
            // Update common_ancestor_slot because A is still running
            if let Some(s) = a_tower.last_voted_slot() {
                common_ancestor_slot = s;
                info!("New common_ancestor_slot {}", common_ancestor_slot);
            } else {
                panic!("A's tower has no votes");
            }
            info!("Increase lockout by 6 confirmation levels and save as B's tower");
            a_tower.increase_lockout(6);
            save_tower(&b_ledger_path, &a_tower, &b_info.info.keypair);
            info!("B's new tower: {:?}", a_tower.tower_slots());
        } else {
            panic!("A's tower is missing");
        }

        // Get rid of any slots past common_ancestor_slot
        info!("Removing extra slots from B's blockstore");
        let blockstore = open_blockstore(&b_ledger_path);
        purge_slots_with_count(&blockstore, common_ancestor_slot + 1, 100);
    }

    info!(
        "Run A on majority fork until it reaches slot hash expiry {}",
        solana_sdk::slot_hashes::get_entries()
    );
    let mut last_vote_on_a;
    // Keep A running for a while longer so the majority fork has some decent size
    loop {
        last_vote_on_a = wait_for_last_vote_in_tower_to_land_in_ledger(&a_ledger_path, &a_pubkey);
        if last_vote_on_a
            >= common_ancestor_slot + 2 * (solana_sdk::slot_hashes::get_entries() as u64)
        {
            let blockstore = open_blockstore(&a_ledger_path);
            info!(
                "A majority fork: {:?}",
                AncestorIterator::new(last_vote_on_a, &blockstore).collect::<Vec<Slot>>()
            );
            break;
        }
        sleep(Duration::from_millis(100));
    }

    // Kill A and restart B with voting. B should now fork off
    info!("Killing A");
    let a_info = cluster.exit_node(&a_pubkey);

    info!("Restarting B");
    b_info.config.voting_disabled = false;
    cluster.restart_node(&b_pubkey, b_info, SocketAddrSpace::Unspecified);

    // B will fork off and accumulate enough lockout
    info!("Allowing B to fork");
    loop {
        let blockstore = open_blockstore(&b_ledger_path);
        let last_vote = wait_for_last_vote_in_tower_to_land_in_ledger(&b_ledger_path, &b_pubkey);
        let mut ancestors = AncestorIterator::new(last_vote, &blockstore);
        if let Some(index) = ancestors.position(|x| x == common_ancestor_slot) {
            if index > 7 {
                info!(
                    "B has forked for enough lockout: {:?}",
                    AncestorIterator::new(last_vote, &blockstore).collect::<Vec<Slot>>()
                );
                break;
            }
        }
        sleep(Duration::from_millis(1000));
    }

    info!("Kill B");
    b_info = cluster.exit_node(&b_pubkey);

    info!("Resolve the partition");
    {
        // Here we let B know about the missing blocks that A had produced on its partition
        let a_blockstore = open_blockstore(&a_ledger_path);
        let b_blockstore = open_blockstore(&b_ledger_path);
        copy_blocks(last_vote_on_a, &a_blockstore, &b_blockstore);
    }

    // Now restart A and B and see if B is able to eventually switch onto the majority fork
    info!("Restarting A & B");
    cluster.restart_node(&a_pubkey, a_info, SocketAddrSpace::Unspecified);
    cluster.restart_node(&b_pubkey, b_info, SocketAddrSpace::Unspecified);

    info!("Waiting for B to switch to majority fork and make a root");
    cluster_tests::check_for_new_roots(
        16,
        &[cluster.get_contact_info(&a_pubkey).unwrap().clone()],
        &cluster.connection_cache,
        "test_slot_hashes_expiry",
    );
}

// This test simulates a case where a leader sends a duplicate block with different ancestory. One
// version builds off of the rooted path, however the other version builds off a pruned branch. The
// validators that receive the pruned version will need to repair in order to continue, which
// requires an ancestor hashes repair.
//
// We setup 3 validators:
// - majority, will produce the rooted path
// - minority, will produce the pruned path
// - our_node, will be fed the pruned version of the duplicate block and need to repair
//
// Additionally we setup 3 observer nodes to propagate votes and participate in the ancestor hashes
// sample.
//
// Fork structure:
//
// 0 - 1 - ... - 10 (fork slot) - 30 - ... - 61 (rooted path) - ...
//                |
//                |- 11 - ... - 29 (pruned path) - 81'
//
//
// Steps:
// 1) Different leader schedule, minority thinks it produces 0-29 and majority rest, majority
//    thinks minority produces all blocks. This is to avoid majority accidentally producing blocks
//    before it shuts down.
// 2) Start cluster, kill our_node.
// 3) Kill majority cluster after it votes for any slot > fork slot (guarantees that the fork slot is
//    reached as minority cannot pass threshold otherwise).
// 4) Let minority produce forks on pruned forks until out of leader slots then kill.
// 5) Truncate majority ledger past fork slot so it starts building off of fork slot.
// 6) Restart majority and wait untill it starts producing blocks on main fork and roots something
//    past the fork slot.
// 7) Construct our ledger by copying majority ledger and copying blocks from minority for the pruned path.
// 8) In our node's ledger, change the parent of the latest slot in majority fork to be the latest
//    slot in the minority fork (simulates duplicate built off of pruned block)
// 9) Start our node which will pruned the minority fork on ledger replay and verify that we can make roots.
//
#[test]
#[serial]
fn test_duplicate_with_pruned_ancestor() {
    solana_logger::setup_with("info,solana_metrics=off");
    solana_core::duplicate_repair_status::set_ancestor_hash_repair_sample_size_for_tests_only(3);

    let majority_leader_stake = 10_000_000 * DEFAULT_NODE_STAKE;
    let minority_leader_stake = 2_000_000 * DEFAULT_NODE_STAKE;
    let our_node = DEFAULT_NODE_STAKE;
    let observer_stake = DEFAULT_NODE_STAKE;

    let slots_per_epoch = 2048;
    let fork_slot: u64 = 10;
    let fork_length: u64 = 20;
    let majority_fork_buffer = 5;

    let mut node_stakes = vec![majority_leader_stake, minority_leader_stake, our_node];
    // We need enough observers to reach `ANCESTOR_HASH_REPAIR_SAMPLE_SIZE`
    node_stakes.append(&mut vec![observer_stake; 3]);

    let num_nodes = node_stakes.len();

    let validator_keys = vec![
        "28bN3xyvrP4E8LwEgtLjhnkb7cY4amQb6DrYAbAYjgRV4GAGgkVM2K7wnxnAS7WDneuavza7x21MiafLu1HkwQt4",
        "2saHBBoTkLMmttmPQP8KfBkcCw45S5cwtV3wTdGCscRC8uxdgvHxpHiWXKx4LvJjNJtnNcbSv5NdheokFFqnNDt8",
        "4mx9yoFBeYasDKBGDWCTWGJdWuJCKbgqmuP8bN9umybCh5Jzngw7KQxe99Rf5uzfyzgba1i65rJW4Wqk7Ab5S8ye",
        "3zsEPEDsjfEay7te9XqNjRTCE7vwuT6u4DHzBJC19yp7GS8BuNRMRjnpVrKCBzb3d44kxc4KPGSHkCmk6tEfswCg",
    ]
    .iter()
    .map(|s| (Arc::new(Keypair::from_base58_string(s)), true))
    .chain(std::iter::repeat_with(|| (Arc::new(Keypair::new()), true)))
    .take(node_stakes.len())
    .collect::<Vec<_>>();
    let validators = validator_keys
        .iter()
        .map(|(kp, _)| kp.pubkey())
        .collect::<Vec<_>>();
    let (majority_pubkey, minority_pubkey, our_node_pubkey) =
        (validators[0], validators[1], validators[2]);

    let mut default_config = ValidatorConfig::default_for_test();
    // Minority fork is leader long enough to create pruned fork
    let validator_to_slots = vec![
        (minority_pubkey, (fork_slot + fork_length) as usize),
        (majority_pubkey, slots_per_epoch as usize),
    ];
    let leader_schedule = create_custom_leader_schedule(validator_to_slots.into_iter());
    default_config.fixed_leader_schedule = Some(FixedSchedule {
        leader_schedule: Arc::new(leader_schedule),
    });

    let mut validator_configs = make_identical_validator_configs(&default_config, num_nodes);
    validator_configs[3].voting_disabled = true;
    // Don't let majority produce anything past the fork by tricking its leader schedule
    validator_configs[0].fixed_leader_schedule = Some(FixedSchedule {
        leader_schedule: Arc::new(create_custom_leader_schedule(
            [(minority_pubkey, slots_per_epoch as usize)].into_iter(),
        )),
    });

    let mut config = ClusterConfig {
        cluster_lamports: DEFAULT_CLUSTER_LAMPORTS + node_stakes.iter().sum::<u64>(),
        node_stakes,
        validator_configs,
        validator_keys: Some(validator_keys),
        slots_per_epoch,
        stakers_slot_offset: slots_per_epoch,
        skip_warmup_slots: true,
        ..ClusterConfig::default()
    };
    let mut cluster = LocalCluster::new(&mut config, SocketAddrSpace::Unspecified);

    let majority_ledger_path = cluster.ledger_path(&majority_pubkey);
    let minority_ledger_path = cluster.ledger_path(&minority_pubkey);
    let our_node_ledger_path = cluster.ledger_path(&our_node_pubkey);

    info!(
        "majority {} ledger path {:?}",
        majority_pubkey, majority_ledger_path
    );
    info!(
        "minority {} ledger path {:?}",
        minority_pubkey, minority_ledger_path
    );
    info!(
        "our_node {} ledger path {:?}",
        our_node_pubkey, our_node_ledger_path
    );

    info!("Killing our node");
    let our_node_info = cluster.exit_node(&our_node_pubkey);

    info!("Waiting on majority validator to vote on at least {fork_slot}");
    let now = Instant::now();
    let mut last_majority_vote = 0;
    loop {
        let elapsed = now.elapsed();
        assert!(
            elapsed <= Duration::from_secs(30),
            "Majority validator failed to vote on a slot >= {} in {} secs,
            majority validator last vote: {}",
            fork_slot,
            elapsed.as_secs(),
            last_majority_vote,
        );
        sleep(Duration::from_millis(100));

        if let Some((last_vote, _)) = last_vote_in_tower(&majority_ledger_path, &majority_pubkey) {
            last_majority_vote = last_vote;
            if last_vote >= fork_slot {
                break;
            }
        }
    }

    info!("Killing majority validator, waiting for minority fork to reach a depth of at least 15",);
    let mut majority_validator_info = cluster.exit_node(&majority_pubkey);

    let now = Instant::now();
    let mut last_minority_vote = 0;
    while last_minority_vote < fork_slot + 15 {
        let elapsed = now.elapsed();
        assert!(
            elapsed <= Duration::from_secs(30),
            "Minority validator failed to create a fork of depth >= {} in {} secs,
            last_minority_vote: {}",
            15,
            elapsed.as_secs(),
            last_minority_vote,
        );

        if let Some((last_vote, _)) = last_vote_in_tower(&minority_ledger_path, &minority_pubkey) {
            last_minority_vote = last_vote;
        }
    }

    info!(
        "Killing minority validator, fork created successfully: {:?}",
        last_minority_vote
    );
    let last_minority_vote =
        wait_for_last_vote_in_tower_to_land_in_ledger(&minority_ledger_path, &minority_pubkey);
    let minority_validator_info = cluster.exit_node(&minority_pubkey);

    info!("Truncating majority validator ledger to {fork_slot}");
    {
        remove_tower(&majority_ledger_path, &majority_pubkey);
        let blockstore = open_blockstore(&majority_ledger_path);
        purge_slots_with_count(&blockstore, fork_slot + 1, 100);
    }

    info!("Restarting majority validator");
    // Make sure we don't send duplicate votes
    majority_validator_info.config.wait_to_vote_slot = Some(fork_slot + fork_length);
    // Fix the leader schedule so we can produce blocks
    majority_validator_info.config.fixed_leader_schedule =
        minority_validator_info.config.fixed_leader_schedule.clone();
    cluster.restart_node(
        &majority_pubkey,
        majority_validator_info,
        SocketAddrSpace::Unspecified,
    );

    let mut last_majority_root = 0;
    let now = Instant::now();
    info!(
        "Waiting for majority validator to root something past {}",
        fork_slot + fork_length + majority_fork_buffer
    );
    while last_majority_root <= fork_slot + fork_length + majority_fork_buffer {
        let elapsed = now.elapsed();
        assert!(
            elapsed <= Duration::from_secs(60),
            "Majority validator failed to root something > {} in {} secs,
            last majority validator vote: {},",
            fork_slot + fork_length + majority_fork_buffer,
            elapsed.as_secs(),
            last_majority_vote,
        );
        sleep(Duration::from_millis(100));

        if let Some(last_root) = last_root_in_tower(&majority_ledger_path, &majority_pubkey) {
            last_majority_root = last_root;
        }
    }

    let last_majority_vote =
        wait_for_last_vote_in_tower_to_land_in_ledger(&majority_ledger_path, &majority_pubkey);
    info!("Creating duplicate block built off of pruned branch for our node. Last majority vote {last_majority_vote}, Last minority vote {last_minority_vote}");
    {
        {
            // Copy majority fork
            std::fs::remove_dir_all(&our_node_info.info.ledger_path).unwrap();
            let mut opt = fs_extra::dir::CopyOptions::new();
            opt.copy_inside = true;
            fs_extra::dir::copy(&majority_ledger_path, &our_node_ledger_path, &opt).unwrap();
            remove_tower(&our_node_ledger_path, &majority_pubkey);
        }

        // Copy minority fork. Rewind our root so that we can copy over the purged bank
        let minority_blockstore = open_blockstore(&minority_validator_info.info.ledger_path);
        let mut our_blockstore = open_blockstore(&our_node_info.info.ledger_path);
        our_blockstore.set_last_root(fork_slot - 1);
        copy_blocks(last_minority_vote, &minority_blockstore, &our_blockstore);

        // Change last block parent to chain off of (purged) minority fork
        info!("For our node, changing parent of {last_majority_vote} to {last_minority_vote}");
        purge_slots_with_count(&our_blockstore, last_majority_vote, 1);
        our_blockstore.add_tree(
            tr(last_minority_vote) / tr(last_majority_vote),
            false,
            true,
            64,
            Hash::default(),
        );

        // Update the root to set minority fork back as pruned
        our_blockstore.set_last_root(fork_slot + fork_length);
    }

    // Actual test, `our_node` will replay the minority fork, then the majority fork which will
    // prune the minority fork. Then finally the problematic block will be skipped (not replayed)
    // because its parent has been pruned from bank forks. Meanwhile the majority validator has
    // continued making blocks and voting, duplicate confirming everything. This will cause the
    // pruned fork to become popular triggering an ancestor hashes repair, eventually allowing our
    // node to dump & repair & continue making roots.
    info!("Restarting our node, verifying that our node is making roots past the duplicate block");

    cluster.restart_node(
        &our_node_pubkey,
        our_node_info,
        SocketAddrSpace::Unspecified,
    );

    cluster_tests::check_for_new_roots(
        16,
        &[cluster.get_contact_info(&our_node_pubkey).unwrap().clone()],
        &cluster.connection_cache,
        "test_duplicate_with_pruned_ancestor",
    );
}
