#[macro_use]
extern crate solana_core;
extern crate crossbeam_channel;

use crossbeam_channel::unbounded;
use log::*;
use rand::{thread_rng, Rng};
use rayon::prelude::*;
use solana_core::bank_forks::BankForks;
use solana_core::banking_stage::{create_test_recorder, BankingStage};
use solana_core::blocktree::{get_tmp_ledger_path, Blocktree};
use solana_core::cluster_info::ClusterInfo;
use solana_core::cluster_info::Node;
use solana_core::genesis_utils::{create_genesis_block, GenesisBlockInfo};
use solana_core::packet::to_packets_chunked;
use solana_core::poh_recorder::PohRecorder;
use solana_core::poh_recorder::WorkingBankEntry;
use solana_core::service::Service;
use solana_measure::measure::Measure;
use solana_runtime::bank::Bank;
use solana_sdk::hash::Hash;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Keypair;
use solana_sdk::signature::Signature;
use solana_sdk::system_transaction;
use solana_sdk::timing::{duration_as_us, timestamp};
use solana_sdk::transaction::Transaction;
use std::iter;
use std::sync::atomic::Ordering;
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex, RwLock};
use std::thread::sleep;
use std::time::{Duration, Instant};

fn check_txs(
    receiver: &Arc<Receiver<WorkingBankEntry>>,
    ref_tx_count: usize,
    poh_recorder: &Arc<Mutex<PohRecorder>>,
) -> bool {
    let mut total = 0;
    let now = Instant::now();
    let mut no_bank = false;
    loop {
        if let Ok((_bank, (entry, _tick_height))) = receiver.recv_timeout(Duration::from_millis(10))
        {
            total += entry.transactions.len();
        }
        if total >= ref_tx_count {
            break;
        }
        if now.elapsed().as_secs() > 60 {
            break;
        }
        if poh_recorder.lock().unwrap().bank().is_none() {
            trace!("no bank");
            no_bank = true;
            break;
        }
    }
    if !no_bank {
        assert!(total >= ref_tx_count);
    }
    no_bank
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

struct Config {
    packets_per_batch: usize,
    chunk_len: usize,
    num_threads: usize,
}

impl Config {
    fn get_transactions_index(&self, chunk_index: usize) -> usize {
        chunk_index * (self.chunk_len / self.num_threads) * self.packets_per_batch
    }
}

fn bytes_as_usize(bytes: &[u8]) -> usize {
    bytes[0] as usize | (bytes[1] as usize) << 8
}

fn main() {
    solana_logger::setup();
    let num_threads = BankingStage::num_threads() as usize;
    //   a multiple of packet chunk duplicates to avoid races
    const CHUNKS: usize = 8 * 2;
    const PACKETS_PER_BATCH: usize = 192;
    let txes = PACKETS_PER_BATCH * num_threads * CHUNKS;
    let mint_total = 1_000_000_000_000;
    let GenesisBlockInfo {
        genesis_block,
        mint_keypair,
        ..
    } = create_genesis_block(mint_total);

    let (verified_sender, verified_receiver) = unbounded();
    let (vote_sender, vote_receiver) = unbounded();
    let bank0 = Bank::new(&genesis_block);
    let mut bank_forks = BankForks::new(0, bank0);
    let mut bank = bank_forks.working_bank();

    info!("threads: {} txs: {}", num_threads, txes);

    let mut transactions = make_accounts_txs(txes, &mint_keypair, genesis_block.hash());

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
    let mut verified: Vec<_> = to_packets_chunked(&transactions.clone(), PACKETS_PER_BATCH)
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
            create_test_recorder(&bank, &blocktree, None);
        let cluster_info = ClusterInfo::new_with_invalid_keypair(Node::new_localhost().info);
        let cluster_info = Arc::new(RwLock::new(cluster_info));
        let banking_stage = BankingStage::new(
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
        let mut total = 0;
        let mut tx_total = 0;
        let mut txs_processed = 0;
        let mut root = 1;
        let collector = Pubkey::new_rand();
        const ITERS: usize = 1_000;
        let config = Config {
            packets_per_batch: PACKETS_PER_BATCH,
            chunk_len,
            num_threads,
        };
        for _ in 0..ITERS {
            let now = Instant::now();
            let mut sent = 0;

            for (i, v) in verified[start..start + chunk_len]
                .chunks(chunk_len / num_threads)
                .enumerate()
            {
                let mut byte = 0;
                let index = config.get_transactions_index(start + i);
                if index < transactions.len() {
                    byte = bytes_as_usize(transactions[index].signatures[0].as_ref());
                }
                trace!(
                    "sending... {}..{} {} v.len: {} sig: {} transactions.len: {} index: {}",
                    start + i,
                    start + chunk_len,
                    timestamp(),
                    v.len(),
                    byte,
                    transactions.len(),
                    index,
                );
                for xv in v {
                    sent += xv.0.packets.len();
                }
                verified_sender.send(v.to_vec()).unwrap();
            }
            let start_tx_index = config.get_transactions_index(start);
            let end_tx_index = config.get_transactions_index(start + chunk_len);
            for tx in &transactions[start_tx_index..end_tx_index] {
                loop {
                    if bank.get_signature_status(&tx.signatures[0]).is_some() {
                        break;
                    }
                    if poh_recorder.lock().unwrap().bank().is_none() {
                        break;
                    }
                    sleep(Duration::from_millis(5));
                }
            }
            if check_txs(&signal_receiver2, txes / CHUNKS, &poh_recorder) {
                debug!(
                    "resetting bank {} tx count: {} txs_proc: {}",
                    bank.slot(),
                    bank.transaction_count(),
                    txs_processed
                );
                assert!(txs_processed < bank.transaction_count());
                txs_processed = bank.transaction_count();
                tx_total += duration_as_us(&now.elapsed());

                let mut poh_time = Measure::start("poh_time");
                poh_recorder.lock().unwrap().reset(
                    bank.last_blockhash(),
                    bank.slot(),
                    Some((bank.slot(), bank.slot() + 1)),
                );
                poh_time.stop();

                let mut new_bank_time = Measure::start("new_bank");
                let new_bank = Bank::new_from_parent(&bank, &collector, bank.slot() + 1);
                new_bank_time.stop();

                let mut insert_time = Measure::start("insert_time");
                bank_forks.insert(new_bank);
                bank = bank_forks.working_bank();
                insert_time.stop();

                poh_recorder.lock().unwrap().set_bank(&bank);
                assert!(poh_recorder.lock().unwrap().bank().is_some());
                if bank.slot() > 32 {
                    bank_forks.set_root(root, &None);
                    root += 1;
                }
                debug!(
                    "new_bank_time: {}us insert_time: {}us poh_time: {}us",
                    new_bank_time.as_us(),
                    insert_time.as_us(),
                    poh_time.as_us(),
                );
            } else {
                tx_total += duration_as_us(&now.elapsed());
            }

            // This signature clear may not actually clear the signatures
            // in this chunk, but since we rotate between CHUNKS then
            // we should clear them by the time we come around again to re-use that chunk.
            bank.clear_signatures();
            total += duration_as_us(&now.elapsed());
            debug!(
                "time: {} us checked: {} sent: {}",
                duration_as_us(&now.elapsed()),
                txes / CHUNKS,
                sent,
            );

            if bank.slot() > 0 && bank.slot() % 16 == 0 {
                for tx in transactions.iter_mut() {
                    tx.message.recent_blockhash = bank.last_blockhash();
                    let sig: Vec<u8> = (0..64).map(|_| thread_rng().gen()).collect();
                    tx.signatures[0] = Signature::new(&sig[0..64]);
                }
                verified = to_packets_chunked(&transactions.clone(), PACKETS_PER_BATCH)
                    .into_iter()
                    .map(|x| {
                        let len = x.packets.len();
                        (x, iter::repeat(1).take(len).collect())
                    })
                    .collect();
            }

            start += chunk_len;
            start %= verified.len();
        }
        eprintln!(
            "{{'name': 'banking_bench_total', 'median': '{}'}}",
            total / ITERS as u64,
        );
        eprintln!(
            "{{'name': 'banking_bench_tx_total', 'median': '{}'}}",
            tx_total / ITERS as u64,
        );

        drop(verified_sender);
        drop(vote_sender);
        exit.store(true, Ordering::Relaxed);
        banking_stage.join().unwrap();
        debug!("waited for banking_stage");
        poh_service.join().unwrap();
        sleep(Duration::from_secs(1));
        debug!("waited for poh_service");
    }
    let _unused = Blocktree::destroy(&ledger_path);
}
