#![feature(test)]

extern crate test;
use solana_sdk::{
    hash::Hash,
    slot_hashes::{Slot, SlotHashes, MAX_SLOT_HASHES},
    sysvar::Sysvar,
};
use test::Bencher;

#[bench]
fn bench_to_from_account(b: &mut Bencher) {
    let mut slot_hashes = SlotHashes::new(&[]);
    for i in 0..MAX_SLOT_HASHES {
        slot_hashes.add(i as Slot, Hash::default());
    }
    let mut reps = 0;
    b.iter(|| {
        reps += 1;
        let account = slot_hashes.create_account(0);
        slot_hashes = SlotHashes::from_account(&account).unwrap();
    });
}
