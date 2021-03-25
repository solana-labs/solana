#![feature(test)]

extern crate test;
use solana_sdk::{
    account::{create_account_for_test, from_account},
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
<<<<<<< HEAD
        let account = create_account(&slot_hashes, 0);
        slot_hashes = from_account::<SlotHashes>(&account).unwrap();
=======
        let account = create_account_for_test(&slot_hashes);
        slot_hashes = from_account::<SlotHashes, _>(&account).unwrap();
>>>>>>> 6d5c6c17c... Simplify account.rent_epoch handling for sysvar rent (#16049)
    });
}
