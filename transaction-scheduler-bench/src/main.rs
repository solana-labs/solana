use {
    clap::Parser,
    crossbeam_channel::{unbounded, Sender},
    log::*,
    rand::Rng,
    solana_core::transaction_scheduler::{
        SchedulerStage, TransactionScheduler, TransactionSchedulerConfig,
        TransactionSchedulerHandle,
    },
    solana_perf::packet::PacketBatch,
    solana_runtime::{
        bank::Bank,
        bank_forks::BankForks,
        cost_model::CostModel,
        genesis_utils::{create_genesis_config, GenesisConfigInfo},
    },
    solana_sdk::{
        compute_budget::ComputeBudgetInstruction,
        hash::Hash,
        instruction::{AccountMeta, Instruction},
        packet::Packet,
        signature::{Keypair, Signer},
        system_program,
        transaction::{Transaction, VersionedTransaction},
    },
    std::{
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc, RwLock,
        },
        thread::{sleep, spawn, Builder, JoinHandle},
        time::{Duration, Instant},
    },
};

#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
struct Args {
    /// How many packets per second to send to the scheduler
    #[clap(long, env, default_value_t = 200_000)]
    packet_send_rate: usize,

    /// Number of packets per batch
    #[clap(long, env, default_value_t = 128)]
    packets_per_batch: usize,

    /// Number of batches per message
    #[clap(long, env, default_value_t = 4)]
    batches_per_msg: usize,

    /// Number of consuming threads (number of threads requesting batches from scheduler)
    #[clap(long, env, default_value_t = 6)]
    num_execution_threads: usize,

    /// How long each transaction takes to execution in microseconds
    #[clap(long, env, default_value_t = 15)]
    execution_per_tx_us: u64,

    /// Duration of benchmark
    #[clap(long, env, default_value_t = 20.0)]
    duration: f32,

    /// Number of accounts to choose from when signing transactions
    #[clap(long, env, default_value_t = 100000)]
    num_accounts: usize,

    /// Number of read locks per tx
    #[clap(long, env, default_value_t = 4)]
    num_read_locks_per_tx: usize,

    /// Number of write locks per tx
    #[clap(long, env, default_value_t = 2)]
    num_read_write_locks_per_tx: usize,

    /// If true, enables state auction on accounts
    #[clap(long, env)]
    enable_state_auction: bool,
}

struct SchedulerBench {
    tx_sender: Sender<Vec<PacketBatch>>,
    tpu_vote_sender: Sender<Vec<PacketBatch>>,
    gossip_vote_sender: Sender<Vec<PacketBatch>>,
    scheduler: TransactionScheduler,
    bank: Arc<Bank>,
    exit: Arc<AtomicBool>,
}

fn configure_scheduler_bench(config: TransactionSchedulerConfig) -> SchedulerBench {
    let (tx_sender, tx_receiver) = unbounded();
    let (tpu_vote_sender, tpu_vote_receiver) = unbounded();
    let (gossip_vote_sender, gossip_vote_receiver) = unbounded();
    let exit = Arc::new(AtomicBool::new(false));
    let cost_model = Arc::new(RwLock::new(CostModel::default()));

    let scheduler = TransactionScheduler::new(
        tx_receiver,
        tpu_vote_receiver,
        gossip_vote_receiver,
        exit.clone(),
        cost_model,
        config,
    );
    let mint_total = 1_000_000_000_000;
    let GenesisConfigInfo { genesis_config, .. } = create_genesis_config(mint_total);

    let bank0 = Bank::new_for_benches(&genesis_config);
    let bank_forks = BankForks::new(bank0);
    let bank = bank_forks.working_bank();
    bank.write_cost_tracker()
        .unwrap()
        .set_limits(u64::MAX, u64::MAX, u64::MAX);

    SchedulerBench {
        tx_sender,
        tpu_vote_sender,
        gossip_vote_sender,
        scheduler,
        bank,
        exit,
    }
}

fn start_sending_packets(
    packet_send_rate: usize,
    packets_per_batch: usize,
    batches_per_msg: usize,
    blockhash: Hash,
    tx_sender: &Sender<Vec<PacketBatch>>,
    tpu_vote_sender: &Sender<Vec<PacketBatch>>,
    gossip_vote_sender: &Sender<Vec<PacketBatch>>,
    exit: Arc<AtomicBool>,
    num_accounts: usize,
    num_read_locks_per_tx: usize,
    num_read_write_locks_per_tx: usize,
) -> Vec<JoinHandle<()>> {
    const NUM_SENDERS: usize = 4;

    let tx_sender = tx_sender.clone();
    let _tpu_vote_sender = tpu_vote_sender.clone();
    let _gossip_vote_sender = gossip_vote_sender.clone();

    let packets_per_msg = packets_per_batch * batches_per_msg;
    let loop_freq = packet_send_rate as f64 / packets_per_msg as f64;
    let loop_duration = Duration::from_secs_f64(1.0 / loop_freq) * NUM_SENDERS as u32;

    info!(
        "start_sending_packets packet_send_rate: {} \
        packets_per_batch: {} \
        batches_per_message: {} \
        packets_per_message: {} \
        loop_freq: {}",
        packet_send_rate, packets_per_batch, batches_per_msg, packets_per_msg, loop_freq
    );

    // generate some number of accounts shared across all transactions to mimic account contention
    // Keypair can't be cloned, so reconstruct inside thread
    let accounts = (0..num_accounts).map(|_| Keypair::new().to_base58_string());
    (0..NUM_SENDERS)
        .map(|_| {
            let accounts = accounts.clone();
            let exit = exit.clone();
            let tx_sender = tx_sender.clone();

            Builder::new()
                .name("packet_sender".into())
                .spawn(move || {
                    let keypairs: Vec<_> =
                        accounts.map(|s| Keypair::from_base58_string(&s)).collect();

                    loop {
                        if exit.load(Ordering::Relaxed) {
                            break;
                        }
                        let start = Instant::now();

                        let packet_batches = (0..batches_per_msg).map(|_| {
                            PacketBatch::new(
                                (0..packets_per_batch)
                                    .map(|_| {
                                        let sending_kp = &keypairs
                                            [rand::thread_rng().gen_range(0..keypairs.len())];

                                        let read_account_metas =
                                            (0..num_read_locks_per_tx).map(|_| {
                                                AccountMeta::new_readonly(
                                                    keypairs[rand::thread_rng()
                                                        .gen_range(0..keypairs.len())]
                                                    .pubkey(),
                                                    false,
                                                )
                                            });
                                        let write_account_metas = (0..num_read_write_locks_per_tx)
                                            .map(|_| {
                                                AccountMeta::new(
                                                    keypairs[rand::thread_rng()
                                                        .gen_range(0..keypairs.len())]
                                                    .pubkey(),
                                                    false,
                                                )
                                            });
                                        let ixs = vec![
                                            ComputeBudgetInstruction::set_compute_unit_price(100),
                                            Instruction::new_with_bytes(
                                                system_program::id(),
                                                &[0],
                                                read_account_metas
                                                    .chain(write_account_metas)
                                                    .collect(),
                                            ),
                                        ];

                                        let versioned_tx = VersionedTransaction::from(
                                            Transaction::new_signed_with_payer(
                                                &ixs,
                                                Some(&sending_kp.pubkey()),
                                                &[sending_kp],
                                                blockhash.clone(),
                                            ),
                                        );
                                        Packet::from_data(None, &versioned_tx).unwrap()
                                    })
                                    .collect(),
                            )
                        });

                        packet_batches.into_iter().for_each(|b| {
                            tx_sender.send(vec![b]).unwrap();
                        });

                        let sleep_duration = loop_duration
                            .checked_sub(start.elapsed())
                            .unwrap_or_default();
                        sleep(sleep_duration);
                    }
                })
                .unwrap()
        })
        .collect()
}

fn start_execution_threads(
    scheduler_handle: TransactionSchedulerHandle,
    num_execution_threads: usize,
    execution_per_tx_us: u64,
    exit: Arc<AtomicBool>,
    bank: Arc<Bank>,
    stats_sender: Sender<usize>,
) -> Vec<JoinHandle<()>> {
    (0..num_execution_threads)
        .map(|t_id| {
            let exit = exit.clone();
            let scheduler_handle = scheduler_handle.clone();
            let bank = bank.clone();
            let stats_sender = stats_sender.clone();

            Builder::new()
                .name(format!("execution_thread_{}", t_id))
                .spawn(move || loop {
                    if exit.load(Ordering::Relaxed) {
                        break;
                    }

                    let batch = scheduler_handle.request_batch(128, &bank).unwrap();

                    let num_txs = batch.sanitized_transactions.len();
                    sleep(Duration::from_micros(
                        (num_txs * execution_per_tx_us as usize) as u64,
                    ));

                    if stats_sender
                        .send(batch.sanitized_transactions.len())
                        .is_err()
                    {
                        break;
                    }

                    let _response = scheduler_handle
                        .send_batch_execution_update(batch.sanitized_transactions, vec![]);
                })
                .unwrap()
        })
        .collect()
}

fn main() {
    // solana_logger::setup_with_default("INFO,solana_core::transaction_scheduler=trace");
    solana_logger::setup_with_default("INFO");

    let Args {
        packet_send_rate,
        packets_per_batch,
        batches_per_msg,
        num_execution_threads,
        execution_per_tx_us,
        duration,
        num_accounts,
        num_read_locks_per_tx,
        num_read_write_locks_per_tx,
        enable_state_auction,
    } = Args::parse();

    let scheduler_config = TransactionSchedulerConfig {
        enable_state_auction,
        backlog_size: 700_000,
    };

    let SchedulerBench {
        tx_sender,
        tpu_vote_sender,
        gossip_vote_sender,
        scheduler,
        bank,
        exit,
    } = configure_scheduler_bench(scheduler_config);

    let sending_handles = start_sending_packets(
        packet_send_rate,
        packets_per_batch,
        batches_per_msg,
        bank.last_blockhash(),
        &tx_sender,
        &tpu_vote_sender,
        &gossip_vote_sender,
        exit.clone(),
        num_accounts,
        num_read_locks_per_tx,
        num_read_write_locks_per_tx,
    );

    let scheduler_handle = scheduler.get_handle(SchedulerStage::Transactions);

    let (stats_sender, stats_receiver) = unbounded();
    let scheduler_stages = start_execution_threads(
        scheduler_handle,
        num_execution_threads,
        execution_per_tx_us,
        exit.clone(),
        bank.clone(),
        stats_sender,
    );

    let stats_thread = spawn(move || {
        let mut count = 0;
        let mut last_log_time = Instant::now();

        for msg in stats_receiver {
            // info!("msg: {}", msg);
            count += msg;

            if last_log_time.elapsed() > Duration::from_secs(1) {
                info!(
                    "txs/s: {}",
                    count as f64 / last_log_time.elapsed().as_secs_f64()
                );
                count = 0;
                last_log_time = Instant::now();
            }
        }
    });

    let _ = spawn(move || {
        sleep(Duration::from_secs_f32(duration));
        exit.store(true, Ordering::Relaxed);
    })
    .join();

    drop(tx_sender);
    drop(tpu_vote_sender);
    drop(gossip_vote_sender);
    let _ = stats_thread.join();
    let _ = scheduler.join().unwrap();
    for h in sending_handles {
        let _ = h.join().unwrap();
    }
    for s in scheduler_stages {
        let _ = s.join().unwrap();
    }
}
