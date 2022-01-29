//! Move flakey tests here so that when they fail, there's less to retry in CI
//! because these tests are run separately from the rest of local cluster tests.
#![allow(clippy::integer_arithmetic)]
use {
    common::{
        copy_blocks, last_vote_in_tower, open_blockstore, purge_slots, remove_tower,
        wait_for_last_vote_in_tower_to_land_in_ledger, RUST_LOG_FILTER,
    },
    log::*,
    serial_test::serial,
    solana_core::validator::ValidatorConfig,
    solana_ledger::{
        ancestor_iterator::AncestorIterator,
        blockstore::Blockstore,
        blockstore_db::{AccessType, BlockstoreOptions},
    },
    solana_local_cluster::{
        cluster::Cluster,
        local_cluster::{ClusterConfig, LocalCluster},
        validator_configs::*,
    },
    solana_sdk::signature::{Keypair, Signer},
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
//
// Step 1: You have validator A + B with 31% and 36% of the stake. Run only validator B:
//
//  S0 -> S1 -> S2 -> S3 (B vote)
//
// Step 2: Turn off B, and truncate the ledger after slot `S3` (simulate votes not
// landing in next slot).
// Copy ledger fully to validator A and validator C
//
// Step 3: Turn on A, and have it vote up to S3. Truncate anything past slot `S3`.
//
//  S0 -> S1 -> S2 -> S3 (A & B vote, optimistically confirmed)
//
// Step 4:
// Start validator C with 33% of the stake with same ledger, but only up to slot S2.
// Have `C` generate some blocks like:
//
// S0 -> S1 -> S2 -> S4
//
// Step 3: Then restart `A` which had 31% of the stake. With the tower, from `A`'s
// perspective it sees:
//
// S0 -> S1 -> S2 -> S3 (voted)
//             |
//             -> S4 -> S5 (C's vote for S4)
//
// The fork choice rule weights look like:
//
// S0 -> S1 -> S2 (ABC) -> S3
//             |
//             -> S4 (C) -> S5
//
// Step 5:
// Without the persisted tower:
//    `A` would choose to vote on the fork with `S4 -> S5`. This is true even if `A`
//    generates a new fork starting at slot `S3` because `C` has more stake than `A`
//    so `A` will eventually pick the fork `C` is on.
//
//    Furthermore `B`'s vote on `S3` is not observable because there are no
//    descendants of slot `S3`, so that fork will not be chosen over `C`'s fork
//
// With the persisted tower:
//    `A` should not be able to generate a switching proof.
//
fn do_test_optimistic_confirmation_violation_with_or_without_tower(with_tower: bool) {
    solana_logger::setup_with_default(RUST_LOG_FILTER);

    // First set up the cluster with 4 nodes
    let slots_per_epoch = 2048;
    let node_stakes = vec![31, 36, 33, 0];

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
    // to the otehr forks, and may violate switching proofs on restart.
    let mut validator_configs =
        make_identical_validator_configs(&ValidatorConfig::default_for_test(), node_stakes.len());

    validator_configs[0].voting_disabled = true;
    validator_configs[2].voting_disabled = true;

    let mut config = ClusterConfig {
        cluster_lamports: 100_000,
        node_stakes,
        validator_configs,
        validator_keys: Some(validator_keys),
        slots_per_epoch,
        stakers_slot_offset: slots_per_epoch,
        skip_warmup_slots: true,
        ..ClusterConfig::default()
    };
    let mut cluster = LocalCluster::new(&mut config, SocketAddrSpace::Unspecified);

    let base_slot = 26; // S2
    let next_slot_on_a = 27; // S3
    let truncated_slots = 100; // just enough to purge all following slots after the S2 and S3

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

    // Immediately kill validator A, and C
    info!("Exiting validators A and C");
    let mut validator_a_info = cluster.exit_node(&validator_a_pubkey);
    let mut validator_c_info = cluster.exit_node(&validator_c_pubkey);

    // Step 1:
    // Let validator B, (D) run for a while.
    let now = Instant::now();
    loop {
        let elapsed = now.elapsed();
        assert!(
            elapsed <= Duration::from_secs(30),
            "Validator B failed to vote on any slot >= {} in {} secs",
            next_slot_on_a,
            elapsed.as_secs()
        );
        sleep(Duration::from_millis(100));

        if let Some((last_vote, _)) = last_vote_in_tower(&val_b_ledger_path, &validator_b_pubkey) {
            if last_vote >= next_slot_on_a {
                break;
            }
        }
    }
    // kill B
    let _validator_b_info = cluster.exit_node(&validator_b_pubkey);

    // Step 2:
    // Stop validator and truncate ledger, copy over B's ledger to A
    info!("truncate validator C's ledger");
    {
        // first copy from validator B's ledger
        std::fs::remove_dir_all(&validator_c_info.info.ledger_path).unwrap();
        let mut opt = fs_extra::dir::CopyOptions::new();
        opt.copy_inside = true;
        fs_extra::dir::copy(&val_b_ledger_path, &val_c_ledger_path, &opt).unwrap();
        // Remove B's tower in the C's new copied ledger
        remove_tower(&val_c_ledger_path, &validator_b_pubkey);

        let blockstore = open_blockstore(&val_c_ledger_path);
        purge_slots(&blockstore, base_slot + 1, truncated_slots);
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
        purge_slots(&a_blockstore, next_slot_on_a + 1, truncated_slots);
    }

    // Step 3:
    // Restart A with voting enabled so that it can vote on B's fork
    // up to `next_slot_on_a`, thereby optimistcally confirming `next_slot_on_a`
    info!("Restarting A");
    validator_a_info.config.voting_disabled = false;
    cluster.restart_node(
        &validator_a_pubkey,
        validator_a_info,
        SocketAddrSpace::Unspecified,
    );

    info!("Waiting for A to vote on slot descended from slot `next_slot_on_a`");
    let now = Instant::now();
    loop {
        if let Some((last_vote_slot, _)) =
            last_vote_in_tower(&val_a_ledger_path, &validator_a_pubkey)
        {
            if last_vote_slot >= next_slot_on_a {
                info!(
                    "Validator A has caught up and voted on slot: {}",
                    last_vote_slot
                );
                break;
            }
        }

        if now.elapsed().as_secs() >= 30 {
            panic!(
                "Validator A has not seen optimistic confirmation slot > {} in 30 seconds",
                next_slot_on_a
            );
        }

        sleep(Duration::from_millis(20));
    }

    info!("Killing A");
    let validator_a_info = cluster.exit_node(&validator_a_pubkey);
    {
        let blockstore = open_blockstore(&val_a_ledger_path);
        purge_slots(&blockstore, next_slot_on_a + 1, truncated_slots);
        if !with_tower {
            info!("Removing tower!");
            remove_tower(&val_a_ledger_path, &validator_a_pubkey);

            // Remove next_slot_on_a from ledger to force validator A to select
            // votes_on_c_fork. Otherwise the validator A will immediately vote
            // for 27 on restart, because it hasn't gotten the heavier fork from
            // validator C yet.
            // Then it will be stuck on 27 unable to switch because C doesn't
            // have enough stake to generate a switching proof
            purge_slots(&blockstore, next_slot_on_a, truncated_slots);
        } else {
            info!("Not removing tower!");
        }
    }

    // Step 4:
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

    // Step 5:
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
            let blockstore = Blockstore::open_with_options(
                &val_a_ledger_path,
                BlockstoreOptions {
                    access_type: AccessType::TryPrimaryThenSecondary,
                    recovery_mode: None,
                    enforce_ulimit_nofile: true,
                },
            )
            .unwrap();
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
