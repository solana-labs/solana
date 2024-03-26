#![feature(test)]
#![allow(clippy::arithmetic_side_effects)]

extern crate test;

use {
    solana_accounts_db::epoch_accounts_hash::EpochAccountsHash,
    solana_runtime::bank::*,
    solana_sdk::{
        account::{AccountSharedData, ReadableAccount},
        genesis_config::create_genesis_config,
        hash::Hash,
        lamports::LamportsError,
        pubkey::Pubkey,
    },
    std::{path::PathBuf, sync::Arc},
    test::Bencher,
};

fn deposit_many(bank: &Bank, pubkeys: &mut Vec<Pubkey>, num: usize) -> Result<(), LamportsError> {
    for t in 0..num {
        let pubkey = solana_sdk::pubkey::new_rand();
        let account =
            AccountSharedData::new((t + 1) as u64, 0, AccountSharedData::default().owner());
        pubkeys.push(pubkey);
        assert!(bank.get_account(&pubkey).is_none());
        test_utils::deposit(bank, &pubkey, (t + 1) as u64)?;
        assert_eq!(bank.get_account(&pubkey).unwrap(), account);
    }
    Ok(())
}

#[bench]
fn bench_accounts_create(bencher: &mut Bencher) {
    let (genesis_config, _) = create_genesis_config(10_000);
    let bank0 = Bank::new_with_paths_for_benches(&genesis_config, vec![PathBuf::from("bench_a0")]);
    bencher.iter(|| {
        let mut pubkeys: Vec<Pubkey> = vec![];
        deposit_many(&bank0, &mut pubkeys, 1000).unwrap();
    });
}

#[bench]
fn bench_accounts_squash(bencher: &mut Bencher) {
    let (mut genesis_config, _) = create_genesis_config(100_000);
    genesis_config.rent.burn_percent = 100; // Avoid triggering an assert in Bank::distribute_rent_to_validators()
    let mut prev_bank = Arc::new(Bank::new_with_paths_for_benches(
        &genesis_config,
        vec![PathBuf::from("bench_a1")],
    ));
    let mut pubkeys: Vec<Pubkey> = vec![];
    deposit_many(&prev_bank, &mut pubkeys, 250_000).unwrap();
    prev_bank.freeze();

    // Need to set the EAH to Valid so that `Bank::new_from_parent()` doesn't panic during
    // freeze when parent is in the EAH calculation window.
    prev_bank
        .rc
        .accounts
        .accounts_db
        .epoch_accounts_hash_manager
        .set_valid(EpochAccountsHash::new(Hash::new_unique()), 0);

    // Measures the performance of the squash operation.
    // This mainly consists of the freeze operation which calculates the
    // merkle hash of the account state and distribution of fees and rent
    let mut slot = 1u64;
    bencher.iter(|| {
        let next_bank = Arc::new(Bank::new_from_parent(
            prev_bank.clone(),
            &Pubkey::default(),
            slot,
        ));
        test_utils::deposit(&next_bank, &pubkeys[0], 1).unwrap();
        next_bank.squash();
        slot += 1;
        prev_bank = next_bank;
    });
}
