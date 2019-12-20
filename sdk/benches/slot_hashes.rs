#![feature(test)]

extern crate test;
use solana_sdk::{
    hash::Hash,
    slot_hashes::{Slot, SlotHashes, MAX_ENTRIES},
    sysvar::Sysvar,
};
use test::Bencher;

#[bench]
fn bench_to_from_account(b: &mut Bencher) {
    let mut slot_hashes = SlotHashes::new(&[]);
    for i in 0..MAX_ENTRIES {
        slot_hashes.add(i as Slot, Hash::default());
    }
    b.iter(|| {
        let account = slot_hashes.create_account(0);
        slot_hashes = SlotHashes::from_account(&account).unwrap();
    });
}
