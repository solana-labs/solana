#![feature(test)]

extern crate solana_core;
extern crate test;

use solana_core::consensus::Tower;
use solana_ledger::bank_forks::BankForks;
use solana_runtime::bank::Bank;
use solana_sdk::{
    pubkey::Pubkey,
    signature::{Keypair, Signer},
};
use std::sync::Arc;
use tempfile::TempDir;
use test::Bencher;

#[bench]
fn bench_save_tower(bench: &mut Bencher) {
    let dir = TempDir::new().unwrap();
    let path = dir.path();

    let vote_account_pubkey = &Pubkey::default();
    let node_keypair = Arc::new(Keypair::new());
    let tower = Tower::new(
        &node_keypair.pubkey(),
        &vote_account_pubkey,
        &BankForks::new(0, Bank::default()),
        &path,
    );

    bench.iter(move || {
        tower.save(&node_keypair).unwrap();
    });
}
