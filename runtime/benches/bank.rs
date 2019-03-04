#![feature(test)]

extern crate test;

use solana_runtime::bank::*;
use solana_sdk::genesis_block::GenesisBlock;
use solana_sdk::hash::hash;
use solana_sdk::signature::{Keypair, KeypairUtil};
use solana_sdk::system_transaction::SystemTransaction;
use solana_sdk::timing::{DEFAULT_TICKS_PER_SLOT, MAX_RECENT_BLOCKHASHES};
use test::Bencher;

#[bench]
fn bench_process_transaction(bencher: &mut Bencher) {
    let (genesis_block, mint_keypair) = GenesisBlock::new(100_000_000);
    let bank = Bank::new(&genesis_block);

    // Create transactions between unrelated parties.
    let transactions: Vec<_> = (0..4096)
        .into_iter()
        .map(|_| {
            // Seed the 'from' account.
            let rando0 = Keypair::new();
            let tx = SystemTransaction::new_move(
                &mint_keypair,
                rando0.pubkey(),
                10_000,
                bank.last_blockhash(),
                0,
            );
            assert_eq!(bank.process_transaction(&tx), Ok(()));

            // Seed the 'to' account and a cell for its signature.
            let rando1 = Keypair::new();
            let tx =
                SystemTransaction::new_move(&rando0, rando1.pubkey(), 1, bank.last_blockhash(), 0);
            assert_eq!(bank.process_transaction(&tx), Ok(()));

            // Finally, return the transaction to the benchmark.
            tx
        })
        .collect();

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
