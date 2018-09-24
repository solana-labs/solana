#![feature(test)]
extern crate bincode;
extern crate rayon;
extern crate solana;
extern crate test;

use rayon::prelude::*;
use solana::bank::Bank;
use solana::banking_stage::BankingStage;
use solana::entry::Entry;
use solana::mint::Mint;
use solana::packet::{to_packets_chunked, PacketRecycler};
use solana::poh_service::PohService;
use solana::signature::{Keypair, KeypairUtil};
use solana::transaction::Transaction;
use std::iter;
use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;
use std::time::Duration;
use test::Bencher;

// use self::test::Bencher;
// use bank::{Bank, MAX_ENTRY_IDS};
// use bincode::serialize;
// use hash::hash;
// use mint::Mint;
// use rayon::prelude::*;
// use signature::{Keypair, KeypairUtil};
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
//             let rando0 = Keypair::new();
//             let tx = Transaction::new(&mint.keypair(), rando0.pubkey(), 1_000, last_id);
//             bank.process_transaction(&tx).unwrap();
//
//             let rando1 = Keypair::new();
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

fn check_txs(receiver: &Receiver<Vec<Entry>>, ref_tx_count: usize) {
    let mut total = 0;
    loop {
        let entries = receiver.recv_timeout(Duration::new(1, 0));
        if let Ok(entries) = entries {
            for entry in &entries {
                total += entry.transactions.len();
            }
        } else {
            break;
        }
        if total >= ref_tx_count {
            break;
        }
    }
    assert_eq!(total, ref_tx_count);
}

#[bench]
fn bench_banking_stage_multi_accounts(bencher: &mut Bencher) {
    let tx = 10_000_usize;
    let mint_total = 1_000_000_000_000;
    let mint = Mint::new(mint_total);
    let num_dst_accounts = 8 * 1024;
    let num_src_accounts = 8 * 1024;

    let srckeys: Vec<_> = (0..num_src_accounts).map(|_| Keypair::new()).collect();
    let dstkeys: Vec<_> = (0..num_dst_accounts)
        .map(|_| Keypair::new().pubkey())
        .collect();

    let transactions: Vec<_> = (0..tx)
        .map(|i| {
            Transaction::new(
                &srckeys[i % num_src_accounts],
                dstkeys[i % num_dst_accounts],
                i as i64,
                mint.last_id(),
            )
        }).collect();

    let (verified_sender, verified_receiver) = channel();
    let (entry_sender, entry_receiver) = channel();
    let packet_recycler = PacketRecycler::default();

    let setup_transactions: Vec<_> = (0..num_src_accounts)
        .map(|i| {
            Transaction::new(
                &mint.keypair(),
                srckeys[i].pubkey(),
                mint_total / num_src_accounts as i64,
                mint.last_id(),
            )
        }).collect();

    bencher.iter(move || {
        let bank = Arc::new(Bank::new(&mint));

        let (hash_sender, hash_receiver) = channel();
        let (_poh_service, poh_receiver) = PohService::new(bank.last_id(), hash_receiver, None);

        let verified_setup: Vec<_> =
            to_packets_chunked(&packet_recycler, &setup_transactions.clone(), tx)
                .into_iter()
                .map(|x| {
                    let len = (x).read().packets.len();
                    (x, iter::repeat(1).take(len).collect())
                }).collect();

        verified_sender.send(verified_setup).unwrap();
        BankingStage::process_packets(
            &bank,
            &hash_sender,
            &poh_receiver,
            &verified_receiver,
            &entry_sender,
        ).unwrap();

        check_txs(&entry_receiver, num_src_accounts);

        let verified: Vec<_> = to_packets_chunked(&packet_recycler, &transactions.clone(), 192)
            .into_iter()
            .map(|x| {
                let len = (x).read().packets.len();
                (x, iter::repeat(1).take(len).collect())
            }).collect();

        verified_sender.send(verified).unwrap();
        BankingStage::process_packets(
            &bank,
            &hash_sender,
            &poh_receiver,
            &verified_receiver,
            &entry_sender,
        ).unwrap();

        check_txs(&entry_receiver, tx);
    });
}

#[bench]
fn bench_banking_stage_single_from(bencher: &mut Bencher) {
    let tx = 10_000_usize;
    let mint = Mint::new(1_000_000_000_000);
    let mut pubkeys = Vec::new();
    let num_keys = 8;
    for _ in 0..num_keys {
        pubkeys.push(Keypair::new().pubkey());
    }

    let transactions: Vec<_> = (0..tx)
        .into_par_iter()
        .map(|i| {
            Transaction::new(
                &mint.keypair(),
                pubkeys[i % num_keys],
                i as i64,
                mint.last_id(),
            )
        }).collect();

    let (verified_sender, verified_receiver) = channel();
    let (entry_sender, entry_receiver) = channel();
    let packet_recycler = PacketRecycler::default();

    bencher.iter(move || {
        let bank = Arc::new(Bank::new(&mint));

        let (hash_sender, hash_receiver) = channel();
        let (_poh_service, poh_receiver) = PohService::new(bank.last_id(), hash_receiver, None);

        let verified: Vec<_> = to_packets_chunked(&packet_recycler, &transactions.clone(), tx)
            .into_iter()
            .map(|x| {
                let len = (x).read().packets.len();
                (x, iter::repeat(1).take(len).collect())
            }).collect();
        verified_sender.send(verified).unwrap();
        BankingStage::process_packets(
            &bank,
            &hash_sender,
            &poh_receiver,
            &verified_receiver,
            &entry_sender,
        ).unwrap();

        check_txs(&entry_receiver, tx);
    });
}
