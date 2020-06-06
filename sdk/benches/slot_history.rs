#![feature(test)]

extern crate test;
use solana_sdk::{slot_history::SlotHistory, sysvar::Sysvar};
use test::Bencher;

#[bench]
fn bench_to_from_account(b: &mut Bencher) {
    let mut slot_history = SlotHistory::default();

    b.iter(|| {
        let account = slot_history.create_account(0);
        slot_history = SlotHistory::from_account(&account).unwrap();
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
