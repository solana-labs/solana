#![feature(test)]
extern crate test;

use {
    rand::Rng,
    solana_sdk::{
        account::AccountSharedData,
        pubkey::Pubkey,
        vote::state::{VoteInit, VoteState, VoteStateVersions},
    },
    solana_vote::vote_account::VoteAccount,
    test::Bencher,
};

fn new_rand_vote_account<R: Rng>(
    rng: &mut R,
    node_pubkey: Option<Pubkey>,
) -> (AccountSharedData, VoteState) {
    let vote_init = VoteInit {
        node_pubkey: node_pubkey.unwrap_or_else(Pubkey::new_unique),
        authorized_voter: Pubkey::new_unique(),
        authorized_withdrawer: Pubkey::new_unique(),
        commission: rng.gen(),
    };
    let clock = solana_sdk::sysvar::clock::Clock {
        slot: rng.gen(),
        epoch_start_timestamp: rng.gen(),
        epoch: rng.gen(),
        leader_schedule_epoch: rng.gen(),
        unix_timestamp: rng.gen(),
    };
    let vote_state = VoteState::new(&vote_init, &clock);
    let account = AccountSharedData::new_data(
        rng.gen(), // lamports
        &VoteStateVersions::new_current(vote_state.clone()),
        &solana_sdk::vote::program::id(), // owner
    )
    .unwrap();
    (account, vote_state)
}

#[bench]
fn bench_vote_account_try_from(b: &mut Bencher) {
    let mut rng = rand::thread_rng();
    let (account, vote_state) = new_rand_vote_account(&mut rng, None);

    b.iter(|| {
        let vote_account = VoteAccount::try_from(account.clone()).unwrap();
        let state = vote_account.vote_state();
        assert_eq!(state, &vote_state);
    });
}
