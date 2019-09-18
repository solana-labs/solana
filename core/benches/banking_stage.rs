#![feature(test)]

extern crate test;
#[macro_use]
extern crate solana_core;

use crossbeam_channel::unbounded;
use log::*;
use rand::{thread_rng, Rng};
use rayon::prelude::*;
use solana_core::banking_stage::{create_test_recorder, BankingStage};
use solana_core::blocktree::{get_tmp_ledger_path, Blocktree};
use solana_core::blocktree_processor::process_entries;
use solana_core::cluster_info::ClusterInfo;
use solana_core::cluster_info::Node;
use solana_core::entry::next_hash;
use solana_core::entry::Entry;
use solana_core::genesis_utils::{create_genesis_block, GenesisBlockInfo};
use solana_core::packet::to_packets_chunked;
use solana_core::poh_recorder::WorkingBankEntry;
use solana_core::service::Service;
use solana_core::test_tx::test_tx;
use solana_runtime::bank::Bank;
use solana_sdk::genesis_block::GenesisBlock;
use solana_sdk::hash::Hash;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Keypair;
use solana_sdk::signature::KeypairUtil;
use solana_sdk::signature::Signature;
use solana_sdk::system_instruction;
use solana_sdk::system_transaction;
use solana_sdk::timing::{duration_as_us, timestamp};
use solana_sdk::transaction::Transaction;
use std::iter;
use std::sync::atomic::Ordering;
use std::sync::mpsc::Receiver;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};
use test::Bencher;

fn check_txs(receiver: &Arc<Receiver<WorkingBankEntry>>, ref_tx_count: usize) {
    let mut total = 0;
    let now = Instant::now();
    loop {
        if let Ok((_bank, (entry, _tick_height))) = receiver.recv_timeout(Duration::new(1, 0)) {
            total += entry.transactions.len();
        }
        if total >= ref_tx_count {
            break;
        }
        if now.elapsed().as_secs() > 60 {
            break;
        }
    }
    assert_eq!(total, ref_tx_count);
}

#[bench]
fn bench_consume_buffered(bencher: &mut Bencher) {
    let GenesisBlockInfo { genesis_block, .. } = create_genesis_block(100_000);
    let bank = Arc::new(Bank::new(&genesis_block));
    let ledger_path = get_tmp_ledger_path!();
    let my_pubkey = Pubkey::new_rand();
    {
        let blocktree = Arc::new(
            Blocktree::open(&ledger_path).expect("Expected to be able to open database ledger"),
        );
        let (exit, poh_recorder, poh_service, _signal_receiver) =
            create_test_recorder(&bank, &blocktree);

        let tx = test_tx();
        let len = 4096;
        let chunk_size = 1024;
        let batches = to_packets_chunked(&vec![tx; len], chunk_size);
        let mut packets = vec![];
        for batch in batches {
            let batch_len = batch.packets.len();
            packets.push((batch, vec![0usize; batch_len]));
        }
        // This tests the performance of buffering packets.
        // If the packet buffers are copied, performance will be poor.
        bencher.iter(move || {
            let _ignored = BankingStage::consume_buffered_packets(
                &my_pubkey,
                &poh_recorder,
                &mut packets,
                10_000,
            );
        });

        exit.store(true, Ordering::Relaxed);
        poh_service.join().unwrap();
    }
    let _unused = Blocktree::destroy(&ledger_path);
}

fn make_accounts_txs(txes: usize, mint_keypair: &Keypair, hash: Hash) -> Vec<Transaction> {
    let to_pubkey = Pubkey::new_rand();
    let dummy = system_transaction::transfer(mint_keypair, &to_pubkey, 1, hash);
    (0..txes)
        .into_par_iter()
        .map(|_| {
            let mut new = dummy.clone();
            let sig: Vec<u8> = (0..64).map(|_| thread_rng().gen()).collect();
            new.message.account_keys[0] = Pubkey::new_rand();
            new.message.account_keys[1] = Pubkey::new_rand();
            new.signatures = vec![Signature::new(&sig[0..64])];
            new
        })
        .collect()
}

fn make_programs_txs(txes: usize, hash: Hash) -> Vec<Transaction> {
    let progs = 4;
    (0..txes)
        .into_iter()
        .map(|_| {
            let mut instructions = vec![];
            let from_key = Keypair::new();
            for _ in 1..progs {
                let to_key = Pubkey::new_rand();
                instructions.push(system_instruction::transfer(&from_key.pubkey(), &to_key, 1));
            }
            let mut new = Transaction::new_unsigned_instructions(instructions);
            new.sign(&[&from_key], hash);
            new
        })
        .collect()
}

enum TransactionType {
    Accounts,
    Programs,
}

fn bench_banking(bencher: &mut Bencher, tx_type: TransactionType) {
    solana_logger::setup();
    let num_threads = BankingStage::num_threads() as usize;
    //   a multiple of packet chunk duplicates to avoid races
    const CHUNKS: usize = 8;
    const PACKETS_PER_BATCH: usize = 192;
    let txes = PACKETS_PER_BATCH * num_threads * CHUNKS;
    let mint_total = 1_000_000_000_000;
    let GenesisBlockInfo {
        mut genesis_block,
        mint_keypair,
        ..
    } = create_genesis_block(mint_total);

    // Set a high ticks_per_slot so we don't run out of ticks
    // during the benchmark
    genesis_block.ticks_per_slot = 10_000;

    let (verified_sender, verified_receiver) = unbounded();
    let (vote_sender, vote_receiver) = unbounded();
    let bank = Arc::new(Bank::new(&genesis_block));

    debug!("threads: {} txs: {}", num_threads, txes);

    let transactions = match tx_type {
        TransactionType::Accounts => make_accounts_txs(txes, &mint_keypair, genesis_block.hash()),
        TransactionType::Programs => make_programs_txs(txes, genesis_block.hash()),
    };

    // fund all the accounts
    transactions.iter().for_each(|tx| {
        let fund = system_transaction::transfer(
            &mint_keypair,
            &tx.message.account_keys[0],
            mint_total / txes as u64,
            genesis_block.hash(),
        );
        let x = bank.process_transaction(&fund);
        x.unwrap();
    });
    //sanity check, make sure all the transactions can execute sequentially
    transactions.iter().for_each(|tx| {
        let res = bank.process_transaction(&tx);
        assert!(res.is_ok(), "sanity test transactions");
    });
    bank.clear_signatures();
    //sanity check, make sure all the transactions can execute in parallel
    let res = bank.process_transactions(&transactions);
    for r in res {
        assert!(r.is_ok(), "sanity parallel execution");
    }
    bank.clear_signatures();
    let verified: Vec<_> = to_packets_chunked(&transactions.clone(), PACKETS_PER_BATCH)
        .into_iter()
        .map(|x| {
            let len = x.packets.len();
            (x, iter::repeat(1).take(len).collect())
        })
        .collect();
    let ledger_path = get_tmp_ledger_path!();
    {
        let blocktree = Arc::new(
            Blocktree::open(&ledger_path).expect("Expected to be able to open database ledger"),
        );
        let (exit, poh_recorder, poh_service, signal_receiver) =
            create_test_recorder(&bank, &blocktree);
        let cluster_info = ClusterInfo::new_with_invalid_keypair(Node::new_localhost().info);
        let cluster_info = Arc::new(RwLock::new(cluster_info));
        let _banking_stage = BankingStage::new(
            &cluster_info,
            &poh_recorder,
            verified_receiver,
            vote_receiver,
        );
        poh_recorder.lock().unwrap().set_bank(&bank);

        let chunk_len = verified.len() / CHUNKS;
        let mut start = 0;

        // This is so that the signal_receiver does not go out of scope after the closure.
        // If it is dropped before poh_service, then poh_service will error when
        // calling send() on the channel.
        let signal_receiver = Arc::new(signal_receiver);
        let signal_receiver2 = signal_receiver.clone();
        bencher.iter(move || {
            let now = Instant::now();
            let mut sent = 0;

            for v in verified[start..start + chunk_len].chunks(chunk_len / num_threads) {
                debug!(
                    "sending... {}..{} {} v.len: {}",
                    start,
                    start + chunk_len,
                    timestamp(),
                    v.len(),
                );
                for xv in v {
                    sent += xv.0.packets.len();
                }
                verified_sender.send(v.to_vec()).unwrap();
            }
            check_txs(&signal_receiver2, txes / CHUNKS);

            // This signature clear may not actually clear the signatures
            // in this chunk, but since we rotate between CHUNKS then
            // we should clear them by the time we come around again to re-use that chunk.
            bank.clear_signatures();
            trace!(
                "time: {} checked: {} sent: {}",
                duration_as_us(&now.elapsed()),
                txes / CHUNKS,
                sent,
            );
            start += chunk_len;
            start %= verified.len();
        });
        drop(vote_sender);
        exit.store(true, Ordering::Relaxed);
        poh_service.join().unwrap();
    }
    let _unused = Blocktree::destroy(&ledger_path);
}

#[bench]
fn bench_banking_stage_multi_accounts(bencher: &mut Bencher) {
    bench_banking(bencher, TransactionType::Accounts);
}

#[bench]
fn bench_banking_stage_multi_programs(bencher: &mut Bencher) {
    bench_banking(bencher, TransactionType::Programs);
}

fn simulate_process_entries(
    randomize_txs: bool,
    mint_keypair: &Keypair,
    mut tx_vector: Vec<Transaction>,
    genesis_block: &GenesisBlock,
    keypairs: &Vec<Keypair>,
    initial_lamports: u64,
    num_accounts: usize,
) {
    let bank = Bank::new(genesis_block);

    for i in 0..(num_accounts / 2) {
        bank.transfer(initial_lamports, mint_keypair, &keypairs[i * 2].pubkey())
            .unwrap();
    }

    for i in (0..num_accounts).step_by(2) {
        tx_vector.push(system_transaction::transfer(
            &keypairs[i],
            &keypairs[i + 1].pubkey(),
            initial_lamports,
            bank.last_blockhash(),
        ));
    }

    // Transfer lamports to each other
    let entry = Entry {
        num_hashes: 1,
        hash: next_hash(&bank.last_blockhash(), 1, &tx_vector),
        transactions: tx_vector,
    };
    process_entries(&bank, &vec![entry], randomize_txs).unwrap();
}

fn bench_process_entries(randomize_txs: bool, bencher: &mut Bencher) {
    // entropy multiplier should be big enough to provide sufficient entropy
    // but small enough to not take too much time while executing the test.
    let entropy_multiplier: usize = 25;
    let initial_lamports = 100;

    // number of accounts need to be in multiple of 4 for correct
    // execution of the test.
    let num_accounts = entropy_multiplier * 4;
    let GenesisBlockInfo {
        genesis_block,
        mint_keypair,
        ..
    } = create_genesis_block((num_accounts + 1) as u64 * initial_lamports);

    let mut keypairs: Vec<Keypair> = vec![];
    let tx_vector: Vec<Transaction> = Vec::with_capacity(num_accounts / 2);

    for _ in 0..num_accounts {
        let keypair = Keypair::new();
        keypairs.push(keypair);
    }

    bencher.iter(|| {
        simulate_process_entries(
            randomize_txs,
            &mint_keypair,
            tx_vector.clone(),
            &genesis_block,
            &keypairs,
            initial_lamports,
            num_accounts,
        );
    });
}

#[bench]
fn bench_process_entries_without_order_shuffeling(bencher: &mut Bencher) {
    bench_process_entries(false, bencher);
}

#[bench]
fn bench_process_entries_with_order_shuffeling(bencher: &mut Bencher) {
    bench_process_entries(true, bencher);
}
