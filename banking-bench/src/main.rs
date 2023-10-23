#![allow(clippy::arithmetic_side_effects)]
use {
    clap::{crate_description, crate_name, Arg, ArgEnum, Command},
    crossbeam_channel::{unbounded, Receiver},
    log::*,
    rand::{thread_rng, Rng},
    rayon::prelude::*,
    solana_client::connection_cache::ConnectionCache,
    solana_core::{
        banking_stage::BankingStage,
        banking_trace::{BankingPacketBatch, BankingTracer, BANKING_TRACE_DIR_DEFAULT_BYTE_LIMIT},
    },
    solana_gossip::cluster_info::{ClusterInfo, Node},
    solana_ledger::{
        blockstore::Blockstore,
        genesis_utils::{create_genesis_config, GenesisConfigInfo},
        get_tmp_ledger_path,
        leader_schedule_cache::LeaderScheduleCache,
    },
    solana_measure::measure::Measure,
    solana_perf::packet::{to_packet_batches, PacketBatch},
    solana_poh::poh_recorder::{create_test_recorder, PohRecorder, WorkingBankEntry},
    solana_runtime::{
        bank::Bank, bank_forks::BankForks, prioritization_fee_cache::PrioritizationFeeCache,
    },
    solana_sdk::{
        compute_budget::ComputeBudgetInstruction,
        hash::Hash,
        message::Message,
        pubkey::{self, Pubkey},
        signature::{Keypair, Signature, Signer},
        system_instruction, system_transaction,
        timing::{duration_as_us, timestamp},
        transaction::Transaction,
    },
    solana_streamer::socket::SocketAddrSpace,
    solana_tpu_client::tpu_client::DEFAULT_TPU_CONNECTION_POOL_SIZE,
    std::{
        sync::{atomic::Ordering, Arc, RwLock},
        thread::sleep,
        time::{Duration, Instant},
    },
};

// transfer transaction cost = 1 * SIGNATURE_COST +
//                             2 * WRITE_LOCK_UNITS +
//                             1 * system_program
//                           = 1470 CU
const TRANSFER_TRANSACTION_COST: u32 = 1470;

fn check_txs(
    receiver: &Arc<Receiver<WorkingBankEntry>>,
    ref_tx_count: usize,
    poh_recorder: &Arc<RwLock<PohRecorder>>,
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
        if poh_recorder.read().unwrap().bank().is_none() {
            no_bank = true;
            break;
        }
    }
    if !no_bank {
        assert!(total >= ref_tx_count);
    }
    no_bank
}

#[derive(ArgEnum, Clone, Copy, PartialEq, Eq)]
enum WriteLockContention {
    /// No transactions lock the same accounts.
    None,
    /// Transactions don't lock the same account, unless they belong to the same batch.
    SameBatchOnly,
    /// All transactions write lock the same account.
    Full,
}

impl WriteLockContention {
    fn possible_values<'a>() -> impl Iterator<Item = clap::PossibleValue<'a>> {
        Self::value_variants()
            .iter()
            .filter_map(|v| v.to_possible_value())
    }
}

impl std::str::FromStr for WriteLockContention {
    type Err = String;
    fn from_str(input: &str) -> Result<Self, String> {
        ArgEnum::from_str(input, false)
    }
}

fn make_accounts_txs(
    total_num_transactions: usize,
    packets_per_batch: usize,
    hash: Hash,
    contention: WriteLockContention,
    simulate_mint: bool,
    mint_txs_percentage: usize,
) -> Vec<Transaction> {
    let to_pubkey = pubkey::new_rand();
    let chunk_pubkeys: Vec<pubkey::Pubkey> = (0..total_num_transactions / packets_per_batch)
        .map(|_| pubkey::new_rand())
        .collect();
    let payer_key = Keypair::new();
    (0..total_num_transactions)
        .into_par_iter()
        .map(|i| {
            let is_simulated_mint = is_simulated_mint_transaction(
                simulate_mint,
                i,
                packets_per_batch,
                mint_txs_percentage,
            );
            // simulated mint transactions have higher compute-unit-price
            let compute_unit_price = if is_simulated_mint { 5 } else { 1 };
            let mut new = make_transfer_transaction_with_compute_unit_price(
                &payer_key,
                &to_pubkey,
                1,
                hash,
                compute_unit_price,
            );
            let sig: [u8; 64] = std::array::from_fn(|_| thread_rng().gen::<u8>());
            new.message.account_keys[0] = pubkey::new_rand();
            new.message.account_keys[1] = match contention {
                WriteLockContention::None => pubkey::new_rand(),
                WriteLockContention::SameBatchOnly => {
                    // simulated mint transactions have conflict accounts
                    if is_simulated_mint {
                        chunk_pubkeys[i / packets_per_batch]
                    } else {
                        pubkey::new_rand()
                    }
                }
                WriteLockContention::Full => to_pubkey,
            };
            new.signatures = vec![Signature::from(sig)];
            new
        })
        .collect()
}

// In simulating mint, `mint_txs_percentage` transactions in a batch are mint transaction
// (eg., have conflicting account and higher priority) and remaining percentage regular
// transactions (eg., non-conflict and low priority)
fn is_simulated_mint_transaction(
    simulate_mint: bool,
    index: usize,
    packets_per_batch: usize,
    mint_txs_percentage: usize,
) -> bool {
    simulate_mint && (index % packets_per_batch <= packets_per_batch * mint_txs_percentage / 100)
}

fn make_transfer_transaction_with_compute_unit_price(
    from_keypair: &Keypair,
    to: &Pubkey,
    lamports: u64,
    recent_blockhash: Hash,
    compute_unit_price: u64,
) -> Transaction {
    let from_pubkey = from_keypair.pubkey();
    let instructions = vec![
        system_instruction::transfer(&from_pubkey, to, lamports),
        ComputeBudgetInstruction::set_compute_unit_price(compute_unit_price),
        ComputeBudgetInstruction::set_compute_unit_limit(TRANSFER_TRANSACTION_COST),
    ];
    let message = Message::new(&instructions, Some(&from_pubkey));
    Transaction::new(&[from_keypair], message, recent_blockhash)
}

struct PacketsPerIteration {
    packet_batches: Vec<PacketBatch>,
    transactions: Vec<Transaction>,
    packets_per_batch: usize,
}

impl PacketsPerIteration {
    fn new(
        packets_per_batch: usize,
        batches_per_iteration: usize,
        genesis_hash: Hash,
        write_lock_contention: WriteLockContention,
        simulate_mint: bool,
        mint_txs_percentage: usize,
    ) -> Self {
        let total_num_transactions = packets_per_batch * batches_per_iteration;
        let transactions = make_accounts_txs(
            total_num_transactions,
            packets_per_batch,
            genesis_hash,
            write_lock_contention,
            simulate_mint,
            mint_txs_percentage,
        );

        let packet_batches: Vec<PacketBatch> = to_packet_batches(&transactions, packets_per_batch);
        assert_eq!(packet_batches.len(), batches_per_iteration);
        Self {
            packet_batches,
            transactions,
            packets_per_batch,
        }
    }

    fn refresh_blockhash(&mut self, new_blockhash: Hash) {
        for tx in self.transactions.iter_mut() {
            tx.message.recent_blockhash = new_blockhash;
            let sig: [u8; 64] = std::array::from_fn(|_| thread_rng().gen::<u8>());
            tx.signatures[0] = Signature::from(sig);
        }
        self.packet_batches = to_packet_batches(&self.transactions, self.packets_per_batch);
    }
}

#[allow(clippy::cognitive_complexity)]
fn main() {
    solana_logger::setup();

    let matches = Command::new(crate_name!())
        .about(crate_description!())
        .version(solana_version::version!())
        .arg(
            Arg::new("iterations")
                .long("iterations")
                .takes_value(true)
                .help("Number of test iterations"),
        )
        .arg(
            Arg::new("num_chunks")
                .long("num-chunks")
                .takes_value(true)
                .value_name("SIZE")
                .help("Number of transaction chunks."),
        )
        .arg(
            Arg::new("packets_per_batch")
                .long("packets-per-batch")
                .takes_value(true)
                .value_name("SIZE")
                .help("Packets per batch"),
        )
        .arg(
            Arg::new("skip_sanity")
                .long("skip-sanity")
                .takes_value(false)
                .help("Skip transaction sanity execution"),
        )
        .arg(
            Arg::new("trace_banking")
                .long("trace-banking")
                .takes_value(false)
                .help("Enable banking tracing"),
        )
        .arg(
            Arg::new("write_lock_contention")
                .long("write-lock-contention")
                .takes_value(true)
                .possible_values(WriteLockContention::possible_values())
                .help("Accounts that test transactions write lock"),
        )
        .arg(
            Arg::new("batches_per_iteration")
                .long("batches-per-iteration")
                .takes_value(true)
                .help("Number of batches to send in each iteration"),
        )
        .arg(
            Arg::new("num_banking_threads")
                .long("num-banking-threads")
                .takes_value(true)
                .help("Number of threads to use in the banking stage"),
        )
        .arg(
            Arg::new("tpu_disable_quic")
                .long("tpu-disable-quic")
                .takes_value(false)
                .help("Disable forwarding messages to TPU using QUIC"),
        )
        .arg(
            Arg::new("simulate_mint")
                .long("simulate-mint")
                .takes_value(false)
                .help("Simulate mint transactions to have higher priority"),
        )
        .arg(
            Arg::new("mint_txs_percentage")
                .long("mint-txs-percentage")
                .takes_value(true)
                .requires("simulate_mint")
                .help("In simulating mint, number of mint transactions out of 100."),
        )
        .get_matches();

    let num_banking_threads = matches
        .value_of_t::<u32>("num_banking_threads")
        .unwrap_or_else(|_| BankingStage::num_threads());
    //   a multiple of packet chunk duplicates to avoid races
    let num_chunks = matches.value_of_t::<usize>("num_chunks").unwrap_or(16);
    let packets_per_batch = matches
        .value_of_t::<usize>("packets_per_batch")
        .unwrap_or(192);
    let iterations = matches.value_of_t::<usize>("iterations").unwrap_or(1000);
    let batches_per_iteration = matches
        .value_of_t::<usize>("batches_per_iteration")
        .unwrap_or(BankingStage::num_threads() as usize);
    let write_lock_contention = matches
        .value_of_t::<WriteLockContention>("write_lock_contention")
        .unwrap_or(WriteLockContention::None);
    let mint_txs_percentage = matches
        .value_of_t::<usize>("mint_txs_percentage")
        .unwrap_or(99);

    let mint_total = 1_000_000_000_000;
    let GenesisConfigInfo {
        genesis_config,
        mint_keypair,
        ..
    } = create_genesis_config(mint_total);

    let (replay_vote_sender, _replay_vote_receiver) = unbounded();
    let bank0 = Bank::new_for_benches(&genesis_config);
    let bank_forks = BankForks::new_rw_arc(bank0);
    let mut bank = bank_forks.read().unwrap().working_bank();

    // set cost tracker limits to MAX so it will not filter out TXs
    bank.write_cost_tracker()
        .unwrap()
        .set_limits(std::u64::MAX, std::u64::MAX, std::u64::MAX);

    let mut all_packets: Vec<PacketsPerIteration> = std::iter::from_fn(|| {
        Some(PacketsPerIteration::new(
            packets_per_batch,
            batches_per_iteration,
            genesis_config.hash(),
            write_lock_contention,
            matches.is_present("simulate_mint"),
            mint_txs_percentage,
        ))
    })
    .take(num_chunks)
    .collect();

    let total_num_transactions: u64 = all_packets
        .iter()
        .map(|packets_for_single_iteration| packets_for_single_iteration.transactions.len() as u64)
        .sum();
    info!(
        "threads: {} txs: {}",
        num_banking_threads, total_num_transactions
    );

    // fund all the accounts
    all_packets.iter().for_each(|packets_for_single_iteration| {
        packets_for_single_iteration
            .transactions
            .iter()
            .for_each(|tx| {
                let mut fund = system_transaction::transfer(
                    &mint_keypair,
                    &tx.message.account_keys[0],
                    mint_total / total_num_transactions,
                    genesis_config.hash(),
                );
                // Ignore any pesky duplicate signature errors in the case we are using single-payer
                let sig: [u8; 64] = std::array::from_fn(|_| thread_rng().gen::<u8>());
                fund.signatures = vec![Signature::from(sig)];
                bank.process_transaction(&fund).unwrap();
            });
    });

    let skip_sanity = matches.is_present("skip_sanity");
    if !skip_sanity {
        all_packets.iter().for_each(|packets_for_single_iteration| {
            //sanity check, make sure all the transactions can execute sequentially
            packets_for_single_iteration
                .transactions
                .iter()
                .for_each(|tx| {
                    let res = bank.process_transaction(tx);
                    assert!(res.is_ok(), "sanity test transactions error: {res:?}");
                });
        });
        bank.clear_signatures();

        if write_lock_contention == WriteLockContention::None {
            all_packets.iter().for_each(|packets_for_single_iteration| {
                //sanity check, make sure all the transactions can execute in parallel
                let res =
                    bank.process_transactions(packets_for_single_iteration.transactions.iter());
                for r in res {
                    assert!(r.is_ok(), "sanity parallel execution error: {r:?}");
                }
                bank.clear_signatures();
            });
        }
    }

    let ledger_path = get_tmp_ledger_path!();
    {
        let blockstore = Arc::new(
            Blockstore::open(&ledger_path).expect("Expected to be able to open database ledger"),
        );
        let leader_schedule_cache = Arc::new(LeaderScheduleCache::new_from_bank(&bank));
        let (exit, poh_recorder, poh_service, signal_receiver) = create_test_recorder(
            bank.clone(),
            blockstore.clone(),
            None,
            Some(leader_schedule_cache),
        );
        let (banking_tracer, tracer_thread) =
            BankingTracer::new(matches.is_present("trace_banking").then_some((
                &blockstore.banking_trace_path(),
                exit.clone(),
                BANKING_TRACE_DIR_DEFAULT_BYTE_LIMIT,
            )))
            .unwrap();
        let (non_vote_sender, non_vote_receiver) = banking_tracer.create_channel_non_vote();
        let (tpu_vote_sender, tpu_vote_receiver) = banking_tracer.create_channel_tpu_vote();
        let (gossip_vote_sender, gossip_vote_receiver) =
            banking_tracer.create_channel_gossip_vote();
        let cluster_info = {
            let keypair = Arc::new(Keypair::new());
            let node = Node::new_localhost_with_pubkey(&keypair.pubkey());
            ClusterInfo::new(node.info, keypair, SocketAddrSpace::Unspecified)
        };
        let cluster_info = Arc::new(cluster_info);
        let tpu_disable_quic = matches.is_present("tpu_disable_quic");
        let connection_cache = match tpu_disable_quic {
            false => ConnectionCache::new_quic(
                "connection_cache_banking_bench_quic",
                DEFAULT_TPU_CONNECTION_POOL_SIZE,
            ),
            true => ConnectionCache::with_udp(
                "connection_cache_banking_bench_udp",
                DEFAULT_TPU_CONNECTION_POOL_SIZE,
            ),
        };
        let banking_stage = BankingStage::new_thread_local_multi_iterator(
            &cluster_info,
            &poh_recorder,
            non_vote_receiver,
            tpu_vote_receiver,
            gossip_vote_receiver,
            num_banking_threads,
            None,
            replay_vote_sender,
            None,
            Arc::new(connection_cache),
            bank_forks.clone(),
            &Arc::new(PrioritizationFeeCache::new(0u64)),
        );

        // This is so that the signal_receiver does not go out of scope after the closure.
        // If it is dropped before poh_service, then poh_service will error when
        // calling send() on the channel.
        let signal_receiver = Arc::new(signal_receiver);
        let mut total_us = 0;
        let mut tx_total_us = 0;
        let base_tx_count = bank.transaction_count();
        let mut txs_processed = 0;
        let collector = solana_sdk::pubkey::new_rand();
        let mut total_sent = 0;
        for current_iteration_index in 0..iterations {
            trace!("RUNNING ITERATION {}", current_iteration_index);
            let now = Instant::now();
            let mut sent = 0;

            let packets_for_this_iteration = &all_packets[current_iteration_index % num_chunks];
            for (packet_batch_index, packet_batch) in
                packets_for_this_iteration.packet_batches.iter().enumerate()
            {
                sent += packet_batch.len();
                trace!(
                    "Sending PacketBatch index {}, {}",
                    packet_batch_index,
                    timestamp(),
                );
                non_vote_sender
                    .send(BankingPacketBatch::new((vec![packet_batch.clone()], None)))
                    .unwrap();
            }

            for tx in &packets_for_this_iteration.transactions {
                loop {
                    if bank.get_signature_status(&tx.signatures[0]).is_some() {
                        break;
                    }
                    if poh_recorder.read().unwrap().bank().is_none() {
                        break;
                    }
                    sleep(Duration::from_millis(5));
                }
            }

            // check if txs had been processed by bank. Returns when all transactions are
            // processed, with `FALSE` indicate there is still bank. or returns TRUE indicate a
            // bank has expired before receiving all txs.
            if check_txs(
                &signal_receiver,
                packets_for_this_iteration.transactions.len(),
                &poh_recorder,
            ) {
                eprintln!(
                    "[iteration {}, tx sent {}, slot {} expired, bank tx count {}]",
                    current_iteration_index,
                    sent,
                    bank.slot(),
                    bank.transaction_count(),
                );
                tx_total_us += duration_as_us(&now.elapsed());

                let mut poh_time = Measure::start("poh_time");
                poh_recorder
                    .write()
                    .unwrap()
                    .reset(bank.clone(), Some((bank.slot(), bank.slot() + 1)));
                poh_time.stop();

                let mut new_bank_time = Measure::start("new_bank");
                let new_slot = bank.slot() + 1;
                let new_bank = Bank::new_from_parent(bank, &collector, new_slot);
                new_bank_time.stop();

                let mut insert_time = Measure::start("insert_time");
                bank_forks.write().unwrap().insert(new_bank);
                bank = bank_forks.read().unwrap().working_bank();
                insert_time.stop();

                // set cost tracker limits to MAX so it will not filter out TXs
                bank.write_cost_tracker().unwrap().set_limits(
                    std::u64::MAX,
                    std::u64::MAX,
                    std::u64::MAX,
                );

                assert!(poh_recorder.read().unwrap().bank().is_none());
                poh_recorder.write().unwrap().set_bank(bank.clone(), false);
                assert!(poh_recorder.read().unwrap().bank().is_some());
                debug!(
                    "new_bank_time: {}us insert_time: {}us poh_time: {}us",
                    new_bank_time.as_us(),
                    insert_time.as_us(),
                    poh_time.as_us(),
                );
            } else {
                eprintln!(
                    "[iteration {}, tx sent {}, slot {} active, bank tx count {}]",
                    current_iteration_index,
                    sent,
                    bank.slot(),
                    bank.transaction_count(),
                );
                tx_total_us += duration_as_us(&now.elapsed());
            }

            // This signature clear may not actually clear the signatures
            // in this chunk, but since we rotate between CHUNKS then
            // we should clear them by the time we come around again to re-use that chunk.
            bank.clear_signatures();
            total_us += duration_as_us(&now.elapsed());
            total_sent += sent;

            if current_iteration_index % num_chunks == 0 {
                let last_blockhash = bank.last_blockhash();
                for packets_for_single_iteration in all_packets.iter_mut() {
                    packets_for_single_iteration.refresh_blockhash(last_blockhash);
                }
            }
        }
        txs_processed += bank_forks
            .read()
            .unwrap()
            .working_bank()
            .transaction_count();
        debug!("processed: {} base: {}", txs_processed, base_tx_count);

        eprintln!("[total_sent: {}, base_tx_count: {}, txs_processed: {}, txs_landed: {}, total_us: {}, tx_total_us: {}]",
            total_sent, base_tx_count, txs_processed, (txs_processed - base_tx_count), total_us, tx_total_us);

        eprintln!(
            "{{'name': 'banking_bench_total', 'median': '{:.2}'}}",
            (1000.0 * 1000.0 * total_sent as f64) / (total_us as f64),
        );
        eprintln!(
            "{{'name': 'banking_bench_tx_total', 'median': '{:.2}'}}",
            (1000.0 * 1000.0 * total_sent as f64) / (tx_total_us as f64),
        );
        eprintln!(
            "{{'name': 'banking_bench_success_tx_total', 'median': '{:.2}'}}",
            (1000.0 * 1000.0 * (txs_processed - base_tx_count) as f64) / (total_us as f64),
        );

        drop(non_vote_sender);
        drop(tpu_vote_sender);
        drop(gossip_vote_sender);
        exit.store(true, Ordering::Relaxed);
        banking_stage.join().unwrap();
        debug!("waited for banking_stage");
        poh_service.join().unwrap();
        sleep(Duration::from_secs(1));
        debug!("waited for poh_service");
        if let Some(tracer_thread) = tracer_thread {
            tracer_thread.join().unwrap().unwrap();
        }
    }
    let _unused = Blockstore::destroy(&ledger_path);
}
