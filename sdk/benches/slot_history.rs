#![feature(test)]

extern crate test;
use solana_sdk::{slot_history::SlotHistory, sysvar::Sysvar};
use test::Bencher;

#[bench]
fn bench_to_from_account(b: &mut Bencher) {
    let mut slot_history = SlotHistory::new(1 * 1024 * 1024);

    b.iter(|| {
        let account = slot_history.create_account(0);
        slot_history = SlotHistory::from_account(&account).unwrap();
    });
}
