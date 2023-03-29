//! Move flakey tests here so that when they fail, there's less to retry in CI
//! because these tests are run separately from the rest of local cluster tests.
#![allow(clippy::integer_arithmetic)]
use {
    common::*,
    log::*,
    serial_test::serial,
    solana_core::validator::ValidatorConfig,
    solana_ledger::{ancestor_iterator::AncestorIterator, leader_schedule::FixedSchedule},
    solana_local_cluster::{
        cluster::Cluster,
        local_cluster::{ClusterConfig, LocalCluster},
        validator_configs::*,
    },
    solana_sdk::{
        clock::Slot,
        signature::{Keypair, Signer},
    },
    solana_streamer::socket::SocketAddrSpace,
    std::{
        sync::Arc,
        thread::sleep,
        time::{Duration, Instant},
    },
};

mod common;

#[test]
#[serial]
fn test_no_optimistic_confirmation_violation_with_tower() {
    do_test_optimistic_confirmation_violation_with_or_without_tower(true);
}

#[test]
#[serial]
fn test_optimistic_confirmation_violation_without_tower() {
    do_test_optimistic_confirmation_violation_with_or_without_tower(false);
}

// A bit convoluted test case; but this roughly follows this test theoretical scenario:
// Validator A, B, C have 31, 36, 33 % of stake respectively. Leader schedule is split, first half
// of the test B is always leader, second half C is. Additionally we have a non voting validator D with 0
// stake to propagate gossip info.
//
// Step 1: Kill C, only A, B and D should be running
//
//  S0 -> S1 -> S2 -> S3 (A & B vote, optimistically confirmed)
//
// Step 2:
// Kill A and B once we verify that they have voted on S3 or beyond. Copy B's ledger to C but only
// up to slot S2
// Have `C` generate some blocks like:
//
// S0 -> S1 -> S2 -> S4
//
// Step 3: Then restart `A` which had 31% of the stake, and remove S3 from its ledger, so
// that it only sees `C`'s fork at S2. From `A`'s perspective it sees:
//
// S0 -> S1 -> S2
//             |
//             -> S4 -> S5 (C's vote for S4)
//
// The fork choice rule weights look like:
//
// S0 -> S1 -> S2 (ABC)
//             |
//             -> S4 (C) -> S5
//
// Step 5:
// Without the persisted tower:
//    `A` would choose to vote on the fork with `S4 -> S5`.
//
// With the persisted tower:
//    `A` should not be able to generate a switching proof.
//
fn do_test_optimistic_confirmation_violation_with_or_without_tower(with_tower: bool) {
    solana_logger::setup_with("debug");

    // First set up the cluster with 4 nodes
    let slots_per_epoch = 2048;
    let node_stakes = vec![
        31 * DEFAULT_NODE_STAKE,
        36 * DEFAULT_NODE_STAKE,
        33 * DEFAULT_NODE_STAKE,
        0,
    ];

    let base_slot: Slot = 26; // S2
    let next_slot_on_a: Slot = 27; // S3
    let truncated_slots: Slot = 100; // just enough to purge all following slots after the S2 and S3

    // Each pubkeys are prefixed with A, B, C and D.
    // D is needed to:
    // 1) Propagate A's votes for S2 to validator C after A shuts down so that
    // C can avoid NoPropagatedConfirmation errors and continue to generate blocks
    // 2) Provide gossip discovery for `A` when it restarts because `A` will restart
    // at a different gossip port than the entrypoint saved in C's gossip table
    let validator_keys = vec![
        "28bN3xyvrP4E8LwEgtLjhnkb7cY4amQb6DrYAbAYjgRV4GAGgkVM2K7wnxnAS7WDneuavza7x21MiafLu1HkwQt4",
        "2saHBBoTkLMmttmPQP8KfBkcCw45S5cwtV3wTdGCscRC8uxdgvHxpHiWXKx4LvJjNJtnNcbSv5NdheokFFqnNDt8",
        "4mx9yoFBeYasDKBGDWCTWGJdWuJCKbgqmuP8bN9umybCh5Jzngw7KQxe99Rf5uzfyzgba1i65rJW4Wqk7Ab5S8ye",
        "3zsEPEDsjfEay7te9XqNjRTCE7vwuT6u4DHzBJC19yp7GS8BuNRMRjnpVrKCBzb3d44kxc4KPGSHkCmk6tEfswCg",
    ]
    .iter()
    .map(|s| (Arc::new(Keypair::from_base58_string(s)), true))
    .take(node_stakes.len())
    .collect::<Vec<_>>();
    let validators = validator_keys
        .iter()
        .map(|(kp, _)| kp.pubkey())
        .collect::<Vec<_>>();
    let (validator_a_pubkey, validator_b_pubkey, validator_c_pubkey) =
        (validators[0], validators[1], validators[2]);

    // Disable voting on all validators other than validator B to ensure neither of the below two
    // scenarios occur:
    // 1. If the cluster immediately forks on restart while we're killing validators A and C,
    // with Validator B on one side, and `A` and `C` on a heavier fork, it's possible that the lockouts
    // on `A` and `C`'s latest votes do not extend past validator B's latest vote. Then validator B
    // will be stuck unable to vote, but also unable generate a switching proof to the heavier fork.
    //
    // 2. Validator A doesn't vote past `next_slot_on_a` before we can kill it. This is essential
    // because if validator A votes past `next_slot_on_a`, and then we copy over validator B's ledger
    // below only for slots <= `next_slot_on_a`, validator A will not know how it's last vote chains
    // to the other forks, and may violate switching proofs on restart.
    let mut default_config = ValidatorConfig::default_for_test();
    // Split leader schedule 50-50 between validators B and C, don't give validator A any slots because
    // it's going to be deleting its ledger, so may create versions of slots it's already created, but
    // on a different fork.
    let validator_to_slots = vec![
        // Ensure validator b is leader for slots <= `next_slot_on_a`
        (validator_b_pubkey, next_slot_on_a as usize + 1),
        (validator_c_pubkey, next_slot_on_a as usize + 1),
    ];

    let leader_schedule = create_custom_leader_schedule(validator_to_slots.into_iter());
    for slot in 0..=next_slot_on_a {
        assert_eq!(leader_schedule[slot], validator_b_pubkey);
    }

    default_config.fixed_leader_schedule = Some(FixedSchedule {
        leader_schedule: Arc::new(leader_schedule),
    });
    let mut validator_configs =
        make_identical_validator_configs(&default_config, node_stakes.len());

    // Disable voting on validators C, and D
    validator_configs[2].voting_disabled = true;
    validator_configs[3].voting_disabled = true;

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

    let val_a_ledger_path = cluster.ledger_path(&validator_a_pubkey);
    let val_b_ledger_path = cluster.ledger_path(&validator_b_pubkey);
    let val_c_ledger_path = cluster.ledger_path(&validator_c_pubkey);

    info!(
        "val_a {} ledger path {:?}",
        validator_a_pubkey, val_a_ledger_path
    );
    info!(
        "val_b {} ledger path {:?}",
        validator_b_pubkey, val_b_ledger_path
    );
    info!(
        "val_c {} ledger path {:?}",
        validator_c_pubkey, val_c_ledger_path
    );

    // Immediately kill validator C. No need to kill validator A because
    // 1) It has no slots in the leader schedule, so no way to make forks
    // 2) We need it to vote
    info!("Exiting validator C");
    let mut validator_c_info = cluster.exit_node(&validator_c_pubkey);

    // Step 1:
    // Let validator A, B, (D) run. Wait for both `A` and `B` to have voted on `next_slot_on_a` or
    // one of its descendants
    info!(
        "Waiting on both validators A and B to vote on fork at slot {}",
        next_slot_on_a
    );
    let now = Instant::now();
    let mut last_b_vote = 0;
    let mut last_a_vote = 0;
    loop {
        let elapsed = now.elapsed();
        assert!(
            elapsed <= Duration::from_secs(30),
            "One of the validators failed to vote on a slot >= {} in {} secs,
            last validator A vote: {},
            last validator B vote: {}",
            next_slot_on_a,
            elapsed.as_secs(),
            last_a_vote,
            last_b_vote,
        );
        sleep(Duration::from_millis(100));

        if let Some((last_vote, _)) = last_vote_in_tower(&val_b_ledger_path, &validator_b_pubkey) {
            last_b_vote = last_vote;
            if last_vote < next_slot_on_a {
                continue;
            }
        }

        if let Some((last_vote, _)) = last_vote_in_tower(&val_a_ledger_path, &validator_a_pubkey) {
            last_a_vote = last_vote;
            if last_vote >= next_slot_on_a {
                break;
            }
        }
    }

    // kill A and B
    let _validator_b_info = cluster.exit_node(&validator_b_pubkey);
    let validator_a_info = cluster.exit_node(&validator_a_pubkey);

    // Step 2:
    // Truncate ledger, copy over B's ledger to C
    info!("Create validator C's ledger");
    {
        // first copy from validator B's ledger
        std::fs::remove_dir_all(&validator_c_info.info.ledger_path).unwrap();
        let mut opt = fs_extra::dir::CopyOptions::new();
        opt.copy_inside = true;
        fs_extra::dir::copy(&val_b_ledger_path, &val_c_ledger_path, &opt).unwrap();
        // Remove B's tower in C's new copied ledger
        remove_tower(&val_c_ledger_path, &validator_b_pubkey);

        let blockstore = open_blockstore(&val_c_ledger_path);
        purge_slots_with_count(&blockstore, base_slot + 1, truncated_slots);
    }
    info!("Create validator A's ledger");
    {
        // Find latest vote in B, and wait for it to reach blockstore
        let b_last_vote =
            wait_for_last_vote_in_tower_to_land_in_ledger(&val_b_ledger_path, &validator_b_pubkey);

        // Now we copy these blocks to A
        let b_blockstore = open_blockstore(&val_b_ledger_path);
        let a_blockstore = open_blockstore(&val_a_ledger_path);
        copy_blocks(b_last_vote, &b_blockstore, &a_blockstore);

        // Purge uneccessary slots
        purge_slots_with_count(&a_blockstore, next_slot_on_a + 1, truncated_slots);
    }

    // This should be guaranteed because we waited for validator `A` to vote on a slot > `next_slot_on_a`
    // before killing it earlier.
    info!("Checking A's tower for a vote on slot descended from slot `next_slot_on_a`");
    let last_vote_slot = last_vote_in_tower(&val_a_ledger_path, &validator_a_pubkey)
        .unwrap()
        .0;
    assert!(last_vote_slot >= next_slot_on_a);
    info!("Success, A voted on slot {}", last_vote_slot);

    {
        let blockstore = open_blockstore(&val_a_ledger_path);
        if !with_tower {
            info!("Removing tower!");
            remove_tower(&val_a_ledger_path, &validator_a_pubkey);

            // Remove next_slot_on_a from ledger to force validator A to select
            // votes_on_c_fork. Otherwise, in the test case without a tower,
            // the validator A will immediately vote for 27 on restart, because it
            // hasn't gotten the heavier fork from validator C yet.
            // Then it will be stuck on 27 unable to switch because C doesn't
            // have enough stake to generate a switching proof
            purge_slots_with_count(&blockstore, next_slot_on_a, truncated_slots);
        } else {
            info!("Not removing tower!");
        }
    }

    // Step 3:
    // Run validator C only to make it produce and vote on its own fork.
    info!("Restart validator C again!!!");
    validator_c_info.config.voting_disabled = false;
    cluster.restart_node(
        &validator_c_pubkey,
        validator_c_info,
        SocketAddrSpace::Unspecified,
    );

    let mut votes_on_c_fork = std::collections::BTreeSet::new(); // S4 and S5
    for _ in 0..100 {
        sleep(Duration::from_millis(100));

        if let Some((last_vote, _)) = last_vote_in_tower(&val_c_ledger_path, &validator_c_pubkey) {
            if last_vote != base_slot {
                votes_on_c_fork.insert(last_vote);
                // Collect 4 votes
                if votes_on_c_fork.len() >= 4 {
                    break;
                }
            }
        }
    }
    assert!(!votes_on_c_fork.is_empty());
    info!("collected validator C's votes: {:?}", votes_on_c_fork);

    // Step 4:
    // verify whether there was violation or not
    info!("Restart validator A again!!!");
    cluster.restart_node(
        &validator_a_pubkey,
        validator_a_info,
        SocketAddrSpace::Unspecified,
    );

    // monitor for actual votes from validator A
    let mut bad_vote_detected = false;
    let mut a_votes = vec![];
    for _ in 0..100 {
        sleep(Duration::from_millis(100));

        if let Some((last_vote, _)) = last_vote_in_tower(&val_a_ledger_path, &validator_a_pubkey) {
            a_votes.push(last_vote);
            let blockstore = open_blockstore(&val_a_ledger_path);
            let mut ancestors = AncestorIterator::new(last_vote, &blockstore);
            if ancestors.any(|a| votes_on_c_fork.contains(&a)) {
                bad_vote_detected = true;
                break;
            }
        }
    }

    info!("Observed A's votes on: {:?}", a_votes);

    // an elaborate way of assert!(with_tower && !bad_vote_detected || ...)
    let expects_optimistic_confirmation_violation = !with_tower;
    if bad_vote_detected != expects_optimistic_confirmation_violation {
        if bad_vote_detected {
            panic!("No violation expected because of persisted tower!");
        } else {
            panic!("Violation expected because of removed persisted tower!");
        }
    } else if bad_vote_detected {
        info!("THIS TEST expected violations. And indeed, there was some, because of removed persisted tower.");
    } else {
        info!("THIS TEST expected no violation. And indeed, there was none, thanks to persisted tower.");
    }
}
