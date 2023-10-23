#![allow(clippy::arithmetic_side_effects)]
#![feature(test)]

use solana_core::validator::BlockProductionMethod;

extern crate test;

use {
    crossbeam_channel::{unbounded, Receiver},
    log::*,
    rand::{thread_rng, Rng},
    rayon::prelude::*,
    solana_client::connection_cache::ConnectionCache,
    solana_core::{
        banking_stage::{
            committer::Committer,
            consumer::Consumer,
            leader_slot_metrics::LeaderSlotMetricsTracker,
            qos_service::QosService,
            unprocessed_packet_batches::*,
            unprocessed_transaction_storage::{ThreadType, UnprocessedTransactionStorage},
            BankingStage, BankingStageStats,
        },
        banking_trace::{BankingPacketBatch, BankingTracer},
    },
    solana_entry::entry::{next_hash, Entry},
    solana_gossip::cluster_info::{ClusterInfo, Node},
    solana_ledger::{
        blockstore::Blockstore,
        blockstore_processor::process_entries_for_tests,
        genesis_utils::{create_genesis_config, GenesisConfigInfo},
        get_tmp_ledger_path,
    },
    solana_perf::{
        packet::{to_packet_batches, Packet},
        test_tx::test_tx,
    },
    solana_poh::poh_recorder::{create_test_recorder, WorkingBankEntry},
    solana_runtime::{
        bank::Bank, bank_forks::BankForks, prioritization_fee_cache::PrioritizationFeeCache,
    },
    solana_sdk::{
        genesis_config::GenesisConfig,
        hash::Hash,
        message::Message,
        pubkey,
        signature::{Keypair, Signature, Signer},
        system_instruction, system_transaction,
        timing::{duration_as_us, timestamp},
        transaction::{Transaction, VersionedTransaction},
    },
    solana_streamer::socket::SocketAddrSpace,
    solana_vote_program::{
        vote_state::VoteStateUpdate, vote_transaction::new_vote_state_update_transaction,
    },
    std::{
        iter::repeat_with,
        sync::{atomic::Ordering, Arc},
        time::{Duration, Instant},
    },
    test::Bencher,
};

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
    let GenesisConfigInfo { genesis_config, .. } = create_genesis_config(100_000);
    let bank = Arc::new(Bank::new_for_benches(&genesis_config));
    let ledger_path = get_tmp_ledger_path!();
    {
        let blockstore = Arc::new(
            Blockstore::open(&ledger_path).expect("Expected to be able to open database ledger"),
        );
        let (exit, poh_recorder, poh_service, _signal_receiver) =
            create_test_recorder(bank, blockstore, None, None);

        let recorder = poh_recorder.read().unwrap().new_recorder();
        let bank_start = poh_recorder.read().unwrap().bank_start().unwrap();

        let tx = test_tx();
        let transactions = vec![tx; 4194304];
        let batches = transactions
            .iter()
            .filter_map(|transaction| {
                let packet = Packet::from_data(None, transaction).ok().unwrap();
                DeserializedPacket::new(packet).ok()
            })
            .collect::<Vec<_>>();
        let batches_len = batches.len();
        let mut transaction_buffer = UnprocessedTransactionStorage::new_transaction_storage(
            UnprocessedPacketBatches::from_iter(batches, 2 * batches_len),
            ThreadType::Transactions,
        );
        let (s, _r) = unbounded();
        let committer = Committer::new(None, s, Arc::new(PrioritizationFeeCache::new(0u64)));
        let consumer = Consumer::new(committer, recorder, QosService::new(1), None);
        // This tests the performance of buffering packets.
        // If the packet buffers are copied, performance will be poor.
        bencher.iter(move || {
            consumer.consume_buffered_packets(
                &bank_start,
                &mut transaction_buffer,
                &BankingStageStats::default(),
                &mut LeaderSlotMetricsTracker::new(0),
            );
        });

        exit.store(true, Ordering::Relaxed);
        poh_service.join().unwrap();
    }
    let _unused = Blockstore::destroy(&ledger_path);
}

fn make_accounts_txs(txes: usize, mint_keypair: &Keypair, hash: Hash) -> Vec<Transaction> {
    let to_pubkey = pubkey::new_rand();
    let dummy = system_transaction::transfer(mint_keypair, &to_pubkey, 1, hash);
    (0..txes)
        .into_par_iter()
        .map(|_| {
            let mut new = dummy.clone();
            let sig: [u8; 64] = std::array::from_fn(|_| thread_rng().gen::<u8>());
            new.message.account_keys[0] = pubkey::new_rand();
            new.message.account_keys[1] = pubkey::new_rand();
            new.signatures = vec![Signature::from(sig)];
            new
        })
        .collect()
}

fn make_programs_txs(txes: usize, hash: Hash) -> Vec<Transaction> {
    let progs = 4;
    (0..txes)
        .map(|_| {
            let from_key = Keypair::new();
            let instructions: Vec<_> = repeat_with(|| {
                let to_key = pubkey::new_rand();
                system_instruction::transfer(&from_key.pubkey(), &to_key, 1)
            })
            .take(progs)
            .collect();
            let message = Message::new(&instructions, Some(&from_key.pubkey()));
            Transaction::new(&[&from_key], message, hash)
        })
        .collect()
}

fn make_vote_txs(txes: usize) -> Vec<Transaction> {
    // 1000 voters
    let num_voters = 1000;
    let (keypairs, vote_keypairs): (Vec<_>, Vec<_>) = (0..num_voters)
        .map(|_| (Keypair::new(), Keypair::new()))
        .unzip();
    (0..txes)
        .map(|i| {
            // Quarter of the votes should be filtered out
            let vote = if i % 4 == 0 {
                VoteStateUpdate::from(vec![(2, 1)])
            } else {
                VoteStateUpdate::from(vec![(i as u64, 1)])
            };
            new_vote_state_update_transaction(
                vote,
                Hash::new_unique(),
                &keypairs[i % num_voters],
                &vote_keypairs[i % num_voters],
                &vote_keypairs[i % num_voters],
                None,
            )
        })
        .collect()
}

enum TransactionType {
    Accounts,
    Programs,
    AccountsAndVotes,
    ProgramsAndVotes,
}

fn bench_banking(bencher: &mut Bencher, tx_type: TransactionType) {
    solana_logger::setup();
    let num_threads = BankingStage::num_threads() as usize;
    //   a multiple of packet chunk duplicates to avoid races
    const CHUNKS: usize = 8;
    const PACKETS_PER_BATCH: usize = 192;
    let txes = PACKETS_PER_BATCH * num_threads * CHUNKS;
    let mint_total = 1_000_000_000_000;
    let GenesisConfigInfo {
        mut genesis_config,
        mint_keypair,
        ..
    } = create_genesis_config(mint_total);

    // Set a high ticks_per_slot so we don't run out of ticks
    // during the benchmark
    genesis_config.ticks_per_slot = 10_000;

    let banking_tracer = BankingTracer::new_disabled();
    let (non_vote_sender, non_vote_receiver) = banking_tracer.create_channel_non_vote();
    let (tpu_vote_sender, tpu_vote_receiver) = banking_tracer.create_channel_tpu_vote();
    let (gossip_vote_sender, gossip_vote_receiver) = banking_tracer.create_channel_gossip_vote();

    let mut bank = Bank::new_for_benches(&genesis_config);
    // Allow arbitrary transaction processing time for the purposes of this bench
    bank.ns_per_slot = u128::MAX;
    let bank_forks = BankForks::new_rw_arc(bank);
    let bank = bank_forks.read().unwrap().get(0).unwrap();

    // set cost tracker limits to MAX so it will not filter out TXs
    bank.write_cost_tracker()
        .unwrap()
        .set_limits(std::u64::MAX, std::u64::MAX, std::u64::MAX);

    debug!("threads: {} txs: {}", num_threads, txes);

    let transactions = match tx_type {
        TransactionType::Accounts | TransactionType::AccountsAndVotes => {
            make_accounts_txs(txes, &mint_keypair, genesis_config.hash())
        }
        TransactionType::Programs | TransactionType::ProgramsAndVotes => {
            make_programs_txs(txes, genesis_config.hash())
        }
    };
    let vote_txs = match tx_type {
        TransactionType::AccountsAndVotes | TransactionType::ProgramsAndVotes => {
            Some(make_vote_txs(txes))
        }
        _ => None,
    };

    // fund all the accounts
    transactions.iter().for_each(|tx| {
        let fund = system_transaction::transfer(
            &mint_keypair,
            &tx.message.account_keys[0],
            mint_total / txes as u64,
            genesis_config.hash(),
        );
        let x = bank.process_transaction(&fund);
        x.unwrap();
    });
    //sanity check, make sure all the transactions can execute sequentially
    transactions.iter().for_each(|tx| {
        let res = bank.process_transaction(tx);
        assert!(res.is_ok(), "sanity test transactions");
    });
    bank.clear_signatures();
    //sanity check, make sure all the transactions can execute in parallel
    let res = bank.process_transactions(transactions.iter());
    for r in res {
        assert!(r.is_ok(), "sanity parallel execution");
    }
    bank.clear_signatures();
    let verified: Vec<_> = to_packet_batches(&transactions, PACKETS_PER_BATCH);
    let vote_packets = vote_txs.map(|vote_txs| {
        let mut packet_batches = to_packet_batches(&vote_txs, PACKETS_PER_BATCH);
        for batch in packet_batches.iter_mut() {
            for packet in batch.iter_mut() {
                packet.meta_mut().set_simple_vote(true);
            }
        }
        packet_batches
    });

    let ledger_path = get_tmp_ledger_path!();
    {
        let blockstore = Arc::new(
            Blockstore::open(&ledger_path).expect("Expected to be able to open database ledger"),
        );
        let (exit, poh_recorder, poh_service, signal_receiver) =
            create_test_recorder(bank.clone(), blockstore, None, None);
        let cluster_info = {
            let keypair = Arc::new(Keypair::new());
            let node = Node::new_localhost_with_pubkey(&keypair.pubkey());
            ClusterInfo::new(node.info, keypair, SocketAddrSpace::Unspecified)
        };
        let cluster_info = Arc::new(cluster_info);
        let (s, _r) = unbounded();
        let _banking_stage = BankingStage::new(
            BlockProductionMethod::ThreadLocalMultiIterator,
            &cluster_info,
            &poh_recorder,
            non_vote_receiver,
            tpu_vote_receiver,
            gossip_vote_receiver,
            None,
            s,
            None,
            Arc::new(ConnectionCache::new("connection_cache_test")),
            bank_forks,
            &Arc::new(PrioritizationFeeCache::new(0u64)),
        );

        let chunk_len = verified.len() / CHUNKS;
        let mut start = 0;

        // This is so that the signal_receiver does not go out of scope after the closure.
        // If it is dropped before poh_service, then poh_service will error when
        // calling send() on the channel.
        let signal_receiver = Arc::new(signal_receiver);
        let signal_receiver2 = signal_receiver;
        bencher.iter(move || {
            let now = Instant::now();
            let mut sent = 0;
            if let Some(vote_packets) = &vote_packets {
                tpu_vote_sender
                    .send(BankingPacketBatch::new((
                        vote_packets[start..start + chunk_len].to_vec(),
                        None,
                    )))
                    .unwrap();
                gossip_vote_sender
                    .send(BankingPacketBatch::new((
                        vote_packets[start..start + chunk_len].to_vec(),
                        None,
                    )))
                    .unwrap();
            }
            for v in verified[start..start + chunk_len].chunks(chunk_len / num_threads) {
                debug!(
                    "sending... {}..{} {} v.len: {}",
                    start,
                    start + chunk_len,
                    timestamp(),
                    v.len(),
                );
                for xv in v {
                    sent += xv.len();
                }
                non_vote_sender
                    .send(BankingPacketBatch::new((v.to_vec(), None)))
                    .unwrap();
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
        exit.store(true, Ordering::Relaxed);
        poh_service.join().unwrap();
    }
    let _unused = Blockstore::destroy(&ledger_path);
}

#[bench]
fn bench_banking_stage_multi_accounts(bencher: &mut Bencher) {
    bench_banking(bencher, TransactionType::Accounts);
}

#[bench]
fn bench_banking_stage_multi_programs(bencher: &mut Bencher) {
    bench_banking(bencher, TransactionType::Programs);
}

#[bench]
fn bench_banking_stage_multi_accounts_with_voting(bencher: &mut Bencher) {
    bench_banking(bencher, TransactionType::AccountsAndVotes);
}

#[bench]
fn bench_banking_stage_multi_programs_with_voting(bencher: &mut Bencher) {
    bench_banking(bencher, TransactionType::ProgramsAndVotes);
}

fn simulate_process_entries(
    mint_keypair: &Keypair,
    mut tx_vector: Vec<VersionedTransaction>,
    genesis_config: &GenesisConfig,
    keypairs: &[Keypair],
    initial_lamports: u64,
    num_accounts: usize,
) {
    let bank = Arc::new(Bank::new_for_benches(genesis_config));

    for i in 0..(num_accounts / 2) {
        bank.transfer(initial_lamports, mint_keypair, &keypairs[i * 2].pubkey())
            .unwrap();
    }

    for i in (0..num_accounts).step_by(2) {
        tx_vector.push(
            system_transaction::transfer(
                &keypairs[i],
                &keypairs[i + 1].pubkey(),
                initial_lamports,
                bank.last_blockhash(),
            )
            .into(),
        );
    }

    // Transfer lamports to each other
    let entry = Entry {
        num_hashes: 1,
        hash: next_hash(&bank.last_blockhash(), 1, &tx_vector),
        transactions: tx_vector,
    };
    process_entries_for_tests(&bank, vec![entry], None, None).unwrap();
}

#[bench]
fn bench_process_entries(bencher: &mut Bencher) {
    // entropy multiplier should be big enough to provide sufficient entropy
    // but small enough to not take too much time while executing the test.
    let entropy_multiplier: usize = 25;
    let initial_lamports = 100;

    // number of accounts need to be in multiple of 4 for correct
    // execution of the test.
    let num_accounts = entropy_multiplier * 4;
    let GenesisConfigInfo {
        genesis_config,
        mint_keypair,
        ..
    } = create_genesis_config((num_accounts + 1) as u64 * initial_lamports);

    let keypairs: Vec<Keypair> = repeat_with(Keypair::new).take(num_accounts).collect();
    let tx_vector: Vec<VersionedTransaction> = Vec::with_capacity(num_accounts / 2);

    bencher.iter(|| {
        simulate_process_entries(
            &mint_keypair,
            tx_vector.clone(),
            &genesis_config,
            &keypairs,
            initial_lamports,
            num_accounts,
        );
    });
}
