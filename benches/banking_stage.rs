extern crate bincode;
#[macro_use]
extern crate criterion;
extern crate rayon;
extern crate solana;

use criterion::{Bencher, Criterion};
use rayon::prelude::*;
use solana::bank::Bank;
use solana::banking_stage::BankingStage;
use solana::mint::Mint;
use solana::packet::{to_packets_chunked, PacketRecycler};
use solana::record_stage::Signal;
use solana::signature::{KeyPair, KeyPairUtil};
use solana::transaction::Transaction;
use std::iter;
use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;

// use self::test::Bencher;
// use bank::{Bank, MAX_ENTRY_IDS};
// use bincode::serialize;
// use hash::hash;
// use mint::Mint;
// use rayon::prelude::*;
// use signature::{KeyPair, KeyPairUtil};
// use std::collections::HashSet;
// use std::time::Instant;
// use transaction::Transaction;
//
// fn bench_process_transactions(_bencher: &mut Bencher) {
//     let mint = Mint::new(100_000_000);
//     let bank = Bank::new(&mint);
//     // Create transactions between unrelated parties.
//     let txs = 100_000;
//     let last_ids: Mutex<HashSet<Hash>> = Mutex::new(HashSet::new());
//     let transactions: Vec<_> = (0..txs)
//         .into_par_iter()
//         .map(|i| {
//             // Seed the 'to' account and a cell for its signature.
//             let dummy_id = i % (MAX_ENTRY_IDS as i32);
//             let last_id = hash(&serialize(&dummy_id).unwrap()); // Semi-unique hash
//             {
//                 let mut last_ids = last_ids.lock().unwrap();
//                 if !last_ids.contains(&last_id) {
//                     last_ids.insert(last_id);
//                     bank.register_entry_id(&last_id);
//                 }
//             }
//
//             // Seed the 'from' account.
//             let rando0 = KeyPair::new();
//             let tx = Transaction::new(&mint.keypair(), rando0.pubkey(), 1_000, last_id);
//             bank.process_transaction(&tx).unwrap();
//
//             let rando1 = KeyPair::new();
//             let tx = Transaction::new(&rando0, rando1.pubkey(), 2, last_id);
//             bank.process_transaction(&tx).unwrap();
//
//             // Finally, return a transaction that's unique
//             Transaction::new(&rando0, rando1.pubkey(), 1, last_id)
//         })
//         .collect();
//
//     let banking_stage = EventProcessor::new(bank, &mint.last_id(), None);
//
//     let now = Instant::now();
//     assert!(banking_stage.process_transactions(transactions).is_ok());
//     let duration = now.elapsed();
//     let sec = duration.as_secs() as f64 + duration.subsec_nanos() as f64 / 1_000_000_000.0;
//     let tps = txs as f64 / sec;
//
//     // Ensure that all transactions were successfully logged.
//     drop(banking_stage.historian_input);
//     let entries: Vec<Entry> = banking_stage.output.lock().unwrap().iter().collect();
//     assert_eq!(entries.len(), 1);
//     assert_eq!(entries[0].transactions.len(), txs as usize);
//
//     println!("{} tps", tps);
// }

fn check_txs(batches: usize, receiver: &Receiver<Signal>, ref_tx_count: usize) {
    let mut total = 0;
    for _ in 0..batches {
        let signal = receiver.recv().unwrap();
        if let Signal::Transactions(transactions) = signal {
            total += transactions.len();
            if total >= ref_tx_count {
                break;
            }
        } else {
            assert!(false);
        }
    }
    assert_eq!(total, ref_tx_count);
}

fn bench_banking_stage_multi_accounts(bencher: &mut Bencher) {
    let tx = 10_000_usize;
    let mint_total = 1_000_000_000_000;
    let mint = Mint::new(mint_total);
    let dst_account_count = 8 * 1024;
    let src_account_count = 8 * 1024;

    let srckeys: Vec<_> = (0..src_account_count).map(|_| KeyPair::new()).collect();
    let dstkeys: Vec<_> = (0..dst_account_count)
        .map(|_| KeyPair::new().pubkey())
        .collect();

    let transactions: Vec<_> = (0..tx)
        .map(|i| {
            Transaction::new(
                &srckeys[i % src_account_count],
                dstkeys[i % dst_account_count],
                i as i64,
                mint.last_id(),
            )
        })
        .collect();

    let (verified_sender, verified_receiver) = channel();
    let (signal_sender, signal_receiver) = channel();
    let packet_recycler = PacketRecycler::default();

    let setup_transactions: Vec<_> = (0..src_account_count)
        .map(|i| {
            Transaction::new(
                &mint.keypair(),
                srckeys[i].pubkey(),
                mint_total / src_account_count as i64,
                mint.last_id(),
            )
        })
        .collect();

    bencher.iter(move || {
        let bank = Arc::new(Bank::new(&mint));

        let verified_setup: Vec<_> =
            to_packets_chunked(&packet_recycler, &setup_transactions.clone(), tx)
                .into_iter()
                .map(|x| {
                    let len = (*x).read().unwrap().packets.len();
                    (x, iter::repeat(1).take(len).collect())
                })
                .collect();

        let verified_setup_len = verified_setup.len();
        verified_sender.send(verified_setup).unwrap();
        BankingStage::process_packets(&bank, &verified_receiver, &signal_sender, &packet_recycler)
            .unwrap();

        check_txs(verified_setup_len, &signal_receiver, src_account_count);

        let verified: Vec<_> = to_packets_chunked(&packet_recycler, &transactions.clone(), 192)
            .into_iter()
            .map(|x| {
                let len = (*x).read().unwrap().packets.len();
                (x, iter::repeat(1).take(len).collect())
            })
            .collect();

        let verified_len = verified.len();
        verified_sender.send(verified).unwrap();
        BankingStage::process_packets(&bank, &verified_receiver, &signal_sender, &packet_recycler)
            .unwrap();

        check_txs(verified_len, &signal_receiver, tx);
    });
}

fn bench_banking_stage_single_from(bencher: &mut Bencher) {
    let tx = 10_000_usize;
    let mint = Mint::new(1_000_000_000_000);
    let mut pubkeys = Vec::new();
    let key_count = 8;
    for _ in 0..key_count {
        pubkeys.push(KeyPair::new().pubkey());
    }

    let transactions: Vec<_> = (0..tx)
        .into_par_iter()
        .map(|i| {
            Transaction::new(
                &mint.keypair(),
                pubkeys[i % key_count],
                i as i64,
                mint.last_id(),
            )
        })
        .collect();

    let (verified_sender, verified_receiver) = channel();
    let (signal_sender, signal_receiver) = channel();
    let packet_recycler = PacketRecycler::default();

    bencher.iter(move || {
        let bank = Arc::new(Bank::new(&mint));
        let verified: Vec<_> = to_packets_chunked(&packet_recycler, &transactions.clone(), tx)
            .into_iter()
            .map(|x| {
                let len = (*x).read().unwrap().packets.len();
                (x, iter::repeat(1).take(len).collect())
            })
            .collect();
        let verified_len = verified.len();
        verified_sender.send(verified).unwrap();
        BankingStage::process_packets(&bank, &verified_receiver, &signal_sender, &packet_recycler)
            .unwrap();

        check_txs(verified_len, &signal_receiver, tx);
    });
}

fn bench(criterion: &mut Criterion) {
    criterion.bench_function("bench_banking_stage_multi_accounts", |bencher| {
        bench_banking_stage_multi_accounts(bencher);
    });
    criterion.bench_function("bench_process_stage_single_from", |bencher| {
        bench_banking_stage_single_from(bencher);
    });
}

criterion_group!(
    name = benches;
    config = Criterion::default().sample_size(2);
    targets = bench
);
criterion_main!(benches);
