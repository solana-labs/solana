#![feature(test)]

extern crate test;

use solana_runtime::bank::*;
use solana_runtime::bank_client::BankClient;
use solana_sdk::client::AsyncClient;
use solana_sdk::genesis_block::GenesisBlock;
use solana_sdk::hash::hash;
use solana_sdk::signature::{Keypair, KeypairUtil};
use solana_sdk::system_transaction;
use solana_sdk::timing::{DEFAULT_TICKS_PER_SLOT, MAX_RECENT_BLOCKHASHES};
use solana_sdk::transaction::Transaction;
use test::Bencher;

// Create transactions between unrelated parties.
pub fn create_sample_transactions(bank: &Bank, mint_keypair: &Keypair) -> Vec<Transaction> {
    (0..4096)
        .into_iter()
        .map(|_| {
            // Seed the 'from' account.
            let rando0 = Keypair::new();
            let tx = system_transaction::transfer(
                &mint_keypair,
                &rando0.pubkey(),
                10_000,
                bank.last_blockhash(),
                0,
            );
            assert_eq!(bank.process_transaction(&tx), Ok(()));

            // Seed the 'to' account and a cell for its signature.
            let rando1 = Keypair::new();
            system_transaction::transfer(&rando0, &rando1.pubkey(), 1, bank.last_blockhash(), 0)
        })
        .collect()
}

#[bench]
fn bench_process_transaction(bencher: &mut Bencher) {
    let (genesis_block, mint_keypair) = GenesisBlock::new(100_000_000);
    let bank = Bank::new(&genesis_block);
    let transactions = create_sample_transactions(&bank, &mint_keypair);

    // Run once to create all the 'to' accounts.
    let results = bank.process_transactions(&transactions);
    assert!(results.iter().all(Result::is_ok));

    let mut id = bank.last_blockhash();
    for _ in 0..(MAX_RECENT_BLOCKHASHES * DEFAULT_TICKS_PER_SLOT as usize) {
        bank.register_tick(&id);
        id = hash(&id.as_ref())
    }

    bencher.iter(|| {
        // Since benchmarker runs this multiple times, we need to clear the signatures.
        bank.clear_signatures();
        let results = bank.process_transactions(&transactions);
        assert!(results.iter().all(Result::is_ok));
    })
}

#[bench]
fn bench_bank_client(bencher: &mut Bencher) {
    let (genesis_block, mint_keypair) = GenesisBlock::new(100_000_000);
    let bank = Bank::new(&genesis_block);
    let transactions = create_sample_transactions(&bank, &mint_keypair);

    bencher.iter(|| {
        let bank = Bank::new(&genesis_block);
        let bank_client = BankClient::new(bank);
        for transaction in transactions.clone() {
            bank_client.async_send_transaction(transaction).unwrap();
        }
    })
}
