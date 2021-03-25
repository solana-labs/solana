#![feature(test)]

extern crate test;
use solana_sdk::{
    account::{create_account_for_test, from_account},
    slot_history::SlotHistory,
};
use test::Bencher;

#[bench]
fn bench_to_from_account(b: &mut Bencher) {
    let mut slot_history = SlotHistory::default();

    b.iter(|| {
<<<<<<< HEAD
        let account = create_account(&slot_history, 0);
        slot_history = from_account::<SlotHistory>(&account).unwrap();
=======
        let account = create_account_for_test(&slot_history);
        slot_history = from_account::<SlotHistory, _>(&account).unwrap();
>>>>>>> 6d5c6c17c... Simplify account.rent_epoch handling for sysvar rent (#16049)
    });
}

#[bench]
fn bench_slot_history_add_new(b: &mut Bencher) {
    let mut slot_history = SlotHistory::default();

    let mut slot = 0;
    b.iter(|| {
        for _ in 0..5 {
            slot_history.add(slot);
            slot += 100_000;
        }
    });
}
