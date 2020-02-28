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
        pubkeys.push(pubkey.clone());
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
    let bank0 = Bank::new_with_paths(&genesis_config, vec![PathBuf::from("bench_a0")]);
    bencher.iter(|| {
        let mut pubkeys: Vec<Pubkey> = vec![];
        deposit_many(&bank0, &mut pubkeys, 1000);
    });
}

#[bench]
fn test_accounts_squash(bencher: &mut Bencher) {
    let (genesis_config, _) = create_genesis_config(100_000);
    let mut banks: Vec<Arc<Bank>> = Vec::with_capacity(10);
    banks.push(Arc::new(Bank::new_with_paths(
        &genesis_config,
        vec![PathBuf::from("bench_a1")],
    )));
    let mut pubkeys: Vec<Pubkey> = vec![];
    deposit_many(&banks[0], &mut pubkeys, 250000);
    banks[0].freeze();
    // Measures the performance of the squash operation merging the accounts
    // with the majority of the accounts present in the parent bank that is
    // moved over to this bank.
    bencher.iter(|| {
        banks.push(Arc::new(Bank::new_from_parent(
            &banks[0],
            &Pubkey::default(),
            1u64,
        )));
        for accounts in 0..10000 {
            banks[1].deposit(&pubkeys[accounts], (accounts + 1) as u64);
        }
        banks[1].squash();
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
