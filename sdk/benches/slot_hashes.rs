#![feature(test)]

extern crate test;
use {
    solana_sdk::{
        account::{create_account_for_test, from_account},
        hash::Hash,
        slot_hashes::{Slot, SlotHashes, MAX_ENTRIES},
    },
    test::Bencher,
};

#[bench]
fn bench_to_from_account(b: &mut Bencher) {
    let mut slot_hashes = SlotHashes::new(&[]);
    for i in 0..MAX_ENTRIES {
        slot_hashes.add(i as Slot, Hash::default());
    }
    b.iter(|| {
        let account = create_account_for_test(&slot_hashes);
        slot_hashes = from_account::<SlotHashes, _>(&account).unwrap();
    });
}
