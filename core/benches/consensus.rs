#![feature(test)]

extern crate solana_core;
extern crate test;

use solana_core::consensus::{Tower, VOTE_THRESHOLD_DEPTH, VOTE_THRESHOLD_SIZE};
use solana_ledger::bank_forks::BankForks;
use solana_runtime::bank::Bank;
use solana_sdk::{
    pubkey::Pubkey,
    signature::{Keypair, KeypairUtil},
};
use tempfile::TempDir;
use test::Bencher;

#[bench]
fn bench_save_tower(bench: &mut Bencher) {
    let dir = TempDir::new().unwrap();
    let path = dir.path().to_path_buf();
    let tower = Tower::new(
        &Pubkey::default(),
        &Pubkey::default(),
        &BankForks::new(0, Bank::default()),
    );
    let keypair = Keypair::new();

    bench.iter(move || {
        tower.save_to_file(&path, &keypair).unwrap();
    });
}
