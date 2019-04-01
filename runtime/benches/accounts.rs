#![feature(test)]

extern crate test;

use solana_runtime::bank::*;
use solana_sdk::account::Account;
use solana_sdk::genesis_block::GenesisBlock;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Keypair, KeypairUtil};
use std::sync::Arc;
use test::Bencher;

fn compare_account(account1: &Account, account2: &Account) -> bool {
    if account1.data != account2.data
        || account1.owner != account2.owner
        || account1.executable != account2.executable
        || account1.lamports != account2.lamports
    {
        return false;
    }
    true
}

fn create_account(bank: &Bank, pubkeys: &mut Vec<Pubkey>, num: usize) {
    for t in 0..num {
        let pubkey = Keypair::new().pubkey();
        let mut default_account = Account::default();
        pubkeys.push(pubkey.clone());
        default_account.lamports = (t + 1) as u64;
        assert!(bank.get_account(&pubkey).is_none());
        bank.deposit(&pubkey, (t + 1) as u64);
        assert_eq!(
            compare_account(&bank.get_account(&pubkey).unwrap(), &default_account),
            true
        );
    }
}

#[bench]
fn test_accounts_create(bencher: &mut Bencher) {
    let (genesis_block, _) = GenesisBlock::new(10_000);
    let bank0 = Bank::new_with_paths(&genesis_block, Some("bench_a0".to_string()));
    bencher.iter(|| {
        let mut pubkeys: Vec<Pubkey> = vec![];
        create_account(&bank0, &mut pubkeys, 1000);
    });
}

#[bench]
fn test_accounts_squash(bencher: &mut Bencher) {
    let (genesis_block, _) = GenesisBlock::new(100_000);
    let mut banks: Vec<Arc<Bank>> = Vec::with_capacity(50);
    banks.push(Arc::new(Bank::new_with_paths(
        &genesis_block,
        Some("bench_a1".to_string()),
    )));
    let mut pubkeys: Vec<Pubkey> = vec![];
    create_account(&banks[0], &mut pubkeys, 250000);
    banks[0].freeze();
    bencher.iter(|| {
        for index in 1..10 {
            banks.push(Arc::new(Bank::new_from_parent(
                &banks[index - 1],
                &Pubkey::default(),
                index as u64,
            )));
            for accounts in 0..10000 {
                banks[index].deposit(&pubkeys[accounts], (accounts + 1) as u64);
            }
            banks[index].squash();
        }
    });
}
