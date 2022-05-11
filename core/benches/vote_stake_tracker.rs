#![feature(test)]

extern crate test;

use {
    solana_core::vote_stake_tracker::{ThresholdCheck, VoteStakeTracker},
    solana_runtime::commitment::VOTE_THRESHOLD_SIZE,
    test::Bencher,
};

pub struct TestVoteThresholds;

impl ThresholdCheck for TestVoteThresholds {
    fn is_duplicate(&self, old_stake: u64, new_stake: u64, total_stake: u64) -> bool {
        let threshold_stake = (total_stake as f64 * VOTE_THRESHOLD_SIZE) as u64;
        old_stake <= threshold_stake && threshold_stake < new_stake
    }

    fn is_vote(&self, old_stake: u64, new_stake: u64, total_stake: u64) -> bool {
        let threshold_stake = (total_stake as f64 * 0.0) as u64;
        old_stake <= threshold_stake && threshold_stake < new_stake
    }
}

// bench result
// old:     675,205 ns/iter (+/- 53,059)
// new:     485,445 ns/iter (+/- 25,743)
#[bench]
fn vote_stake_tracker_bench(bencher: &mut Bencher) {
    bencher.iter(move || {
        let total_epoch_stake = 10;
        let mut vote_stake_tracker = VoteStakeTracker::default();
        const NUM_VOTES: i32 = 2000;

        for _i in 0..NUM_VOTES {
            let pubkey = solana_sdk::pubkey::new_rand();
            let _r = vote_stake_tracker.add_vote_pubkey(
                pubkey,
                1,
                total_epoch_stake,
                TestVoteThresholds {},
            );
            let _stake = vote_stake_tracker.stake();
        }
    });
}
