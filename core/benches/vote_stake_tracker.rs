#![feature(test)]

extern crate test;

use {
    solana_core::vote_stake_tracker::VoteStakeTracker,
    solana_runtime::commitment::VOTE_THRESHOLD_SIZE, test::Bencher,
};

// bench result
// old:     675,205 ns/iter (+/- 53,059)
// new:     506,511 ns/iter (+/- 36,520)
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
                &[VOTE_THRESHOLD_SIZE, 0.0],
            );
            let _stake = vote_stake_tracker.stake();
        }
    });
}
