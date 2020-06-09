#![feature(test)]

extern crate test;

use solana_runtime::{
    accounts::{create_test_accounts, Accounts},
    bank::*,
};
use solana_sdk::{account::Account, genesis_config::create_genesis_config, pubkey::Pubkey};
use std::{path::PathBuf, sync::Arc};
use test::Bencher;

fn deposit_many(bank: &Bank, pubkeys: &mut Vec<Pubkey>, num: usize) {
    for t in 0..num {
        let pubkey = Pubkey::new_rand();
        let account = Account::new((t + 1) as u64, 0, &Account::default().owner);
        pubkeys.push(pubkey);
        assert!(bank.get_account(&pubkey).is_none());
        bank.deposit(&pubkey, (t + 1) as u64);
        assert_eq!(bank.get_account(&pubkey).unwrap(), account);
    }
}

#[bench]
fn bench_has_duplicates(bencher: &mut Bencher) {
    bencher.iter(|| {
        let data = test::black_box([1, 2, 3]);
        assert!(!Accounts::has_duplicates(&data));
    })
}

#[bench]
fn test_accounts_create(bencher: &mut Bencher) {
    let (genesis_config, _) = create_genesis_config(10_000);
    let bank0 = Bank::new_with_paths(&genesis_config, vec![PathBuf::from("bench_a0")], &[]);
    bencher.iter(|| {
        let mut pubkeys: Vec<Pubkey> = vec![];
        deposit_many(&bank0, &mut pubkeys, 1000);
    });
}

#[bench]
fn test_accounts_squash(bencher: &mut Bencher) {
    let (genesis_config, _) = create_genesis_config(100_000);
    let bank1 = Arc::new(Bank::new_with_paths(
        &genesis_config,
        vec![PathBuf::from("bench_a1")],
        &[],
    ));
    let mut pubkeys: Vec<Pubkey> = vec![];
    deposit_many(&bank1, &mut pubkeys, 250_000);
    bank1.freeze();

    // Measures the performance of the squash operation.
    // This mainly consists of the freeze operation which calculates the
    // merkle hash of the account state and distribution of fees and rent
    let mut slot = 1u64;
    bencher.iter(|| {
        let bank2 = Arc::new(Bank::new_from_parent(&bank1, &Pubkey::default(), slot));
        bank2.deposit(&pubkeys[0], 1);
        bank2.squash();
        slot += 1;
    });
}

#[bench]
fn test_accounts_hash_bank_hash(bencher: &mut Bencher) {
    let accounts = Accounts::new(vec![PathBuf::from("bench_accounts_hash_internal")]);
    let mut pubkeys: Vec<Pubkey> = vec![];
    create_test_accounts(&accounts, &mut pubkeys, 60000, 0);
    let ancestors = vec![(0, 0)].into_iter().collect();
    accounts.accounts_db.update_accounts_hash(0, &ancestors);
    bencher.iter(|| assert!(accounts.verify_bank_hash(0, &ancestors)));
}

#[bench]
fn test_update_accounts_hash(bencher: &mut Bencher) {
    solana_logger::setup();
    let accounts = Accounts::new(vec![PathBuf::from("update_accounts_hash")]);
    let mut pubkeys: Vec<Pubkey> = vec![];
    create_test_accounts(&accounts, &mut pubkeys, 50_000, 0);
    let ancestors = vec![(0, 0)].into_iter().collect();
    bencher.iter(|| {
        accounts.accounts_db.update_accounts_hash(0, &ancestors);
    });
}

#[bench]
fn test_accounts_delta_hash(bencher: &mut Bencher) {
    solana_logger::setup();
    let accounts = Accounts::new(vec![PathBuf::from("accounts_delta_hash")]);
    let mut pubkeys: Vec<Pubkey> = vec![];
    create_test_accounts(&accounts, &mut pubkeys, 100_000, 0);
    bencher.iter(|| {
        accounts.accounts_db.get_accounts_delta_hash(0);
    });
}
