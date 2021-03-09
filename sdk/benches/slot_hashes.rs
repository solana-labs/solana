#![feature(test)]

extern crate test;
use solana_sdk::{
    account::{create_account, from_account},
    hash::Hash,
    slot_hashes::{Slot, SlotHashes, MAX_ENTRIES},
};
use test::Bencher;

#[bench]
fn bench_to_from_account(b: &mut Bencher) {
    let mut slot_hashes = SlotHashes::new(&[]);
    for i in 0..MAX_ENTRIES {
        slot_hashes.add(i as Slot, Hash::default());
    }
    b.iter(|| {
        let account = create_account(&slot_hashes, 0);
        slot_hashes = from_account::<SlotHashes, _>(&account).unwrap();
    });
}
