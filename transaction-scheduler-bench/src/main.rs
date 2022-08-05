use {
    clap::Parser,
    crossbeam_channel::{select, Receiver, Sender},
    log::info,
    rand::Rng,
    solana_core::{
        transaction_priority_details::GetTransactionPriorityDetails,
        transaction_scheduler::TransactionScheduler,
    },
    solana_measure::measure,
    solana_perf::packet::{Packet, PacketBatch},
    solana_runtime::{bank::Bank, bank_forks::BankForks},
    solana_sdk::{
        compute_budget::ComputeBudgetInstruction,
        hash::Hash,
        instruction::{AccountMeta, Instruction},
        signature::Keypair,
        signer::Signer,
        system_program,
        transaction::{SanitizedTransaction, Transaction, VersionedTransaction},
    },
    std::{
        sync::{
            atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
            Arc, RwLock,
        },
        thread::{sleep, JoinHandle},
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
    #[clap(long, env, default_value_t = 20)]
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

    /// Max batch size for scheduler
    #[clap(long, env, default_value_t = 128)]
    max_batch_size: usize,

    /// High-conflict sender
    #[clap(long, env, default_value_t = 0)]
    high_conflict_sender: usize,
}

/// Some convenient type aliases
type PreprocessedTransaction = Box<(SanitizedTransaction, Vec<solana_scheduler::LockAttempt>)>;
type TransactionMessage = PreprocessedTransaction;
type CompletedTransactionMessage = solana_scheduler::MultiplexedPayload; // (usize, Box<solana_scheduler::ExecutionEnvironment>); //(usize, TransactionMessage); // thread index and transaction message
type TransactionBatchMessage = Box<solana_scheduler::ExecutionEnvironment>; // Vec<TransactionMessage>;
type BatchSenderMessage = solana_scheduler::MultiplexedPayload; // Vec<Vec<PreprocessedTransaction>>;

#[derive(Debug, Default)]
struct TransactionSchedulerBenchMetrics {
    /// Number of transactions sent to the scheduler
    num_transactions_sent: AtomicUsize,
    /// Number of transactions scheduled
    num_transactions_scheduled: AtomicUsize,
    /// Number of transactions completed
    num_transactions_completed: AtomicUsize,
    /// Priority collected
    priority_collected: AtomicU64,
}

impl TransactionSchedulerBenchMetrics {
    fn report(&self) {
        let num_transactions_sent = self.num_transactions_sent.load(Ordering::Relaxed);
        let num_transactions_scheduled = self.num_transactions_scheduled.load(Ordering::Relaxed);
        let num_transactions_completed = self.num_transactions_completed.load(Ordering::Relaxed);
        let priority_collected = self.priority_collected.load(Ordering::Relaxed);

        let num_transactions_pending = num_transactions_sent - num_transactions_scheduled;
        info!("num_transactions_sent: {num_transactions_sent} num_transactions_pending: {num_transactions_pending} num_transactions_scheduled: {num_transactions_scheduled} num_transactions_completed: {num_transactions_completed} priority_collected: {priority_collected}");
    }
}

struct PacketSendingConfig {
    packets_per_batch: usize,
    batches_per_msg: usize,
    packet_send_rate: usize,
    num_read_locks_per_tx: usize,
    num_write_locks_per_tx: usize,
}

fn spawn_unified_scheduler(
        packet_batch_receiver: Receiver<BatchSenderMessage>,
        transaction_batch_senders: Vec<Sender<TransactionBatchMessage>>,
        //completed_transaction_receiver: Receiver<CompletedTransactionMessage>,
        bank_forks: Arc<RwLock<BankForks>>,
        max_batch_size: usize,
        exit: Arc<AtomicBool>,
) -> JoinHandle<()> {
    std::thread::Builder::new().name("sol-scheduler".to_string()).spawn(move || {
        let mut runnable_queue = solana_scheduler::TaskQueue::default();
        let mut contended_queue = solana_scheduler::TaskQueue::default();
        let mut address_book = solana_scheduler::AddressBook::default();

        solana_scheduler::ScheduleStage::run(
            100,
            &mut runnable_queue,
            &mut contended_queue,
            &mut address_book,
            &packet_batch_receiver.clone(),
            &transaction_batch_senders[0],
            //&completed_transaction_receiver
        );
    }).unwrap()
}

fn main() {
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
        max_batch_size,
        high_conflict_sender,
    } = Args::parse();

    assert!(high_conflict_sender <= num_accounts);

    let (packet_batch_sender, packet_batch_receiver) = crossbeam_channel::unbounded();
    let (transaction_batch_senders, transaction_batch_receivers) =
        build_channels(num_execution_threads);
    let bank_forks = Arc::new(RwLock::new(BankForks::new(Bank::default_for_tests())));
    let exit = Arc::new(AtomicBool::new(false));

    // Spawns and runs the scheduler thread
    let scheduler_handle = spawn_unified_scheduler(
        packet_batch_receiver,
        transaction_batch_senders,
        bank_forks,
        max_batch_size,
        exit.clone(),
    );

    let metrics = Arc::new(TransactionSchedulerBenchMetrics::default());

    // Spawn the execution threads (sleep on transactions and then send completed batches back)
    let execution_handles = start_execution_threads(
        metrics.clone(),
        transaction_batch_receivers,
        packet_batch_sender.clone(),
        execution_per_tx_us,
        exit.clone(),
    );

    // Spawn thread to create and send packet batches
    info!("building accounts...");
    let accounts = Arc::new(build_accounts(num_accounts));
    info!("built accounts...");
    info!("starting packet senders...");
    let duration = Duration::from_secs_f32(duration);
    let packet_sending_config = Arc::new(PacketSendingConfig {
        packets_per_batch,
        batches_per_msg,
        packet_send_rate,
        num_read_locks_per_tx,
        num_write_locks_per_tx: num_read_write_locks_per_tx,
    });
    let packet_sender_handles = spawn_packet_senders(
        metrics.clone(),
        high_conflict_sender,
        accounts,
        packet_batch_sender,
        packet_sending_config,
        duration,
        exit.clone(),
    );

    // Spawn thread for reporting metrics
    std::thread::Builder::new().name("sol-metrics".to_string()).spawn({
        move || {
            let start = Instant::now();
            loop {
                if exit.load(Ordering::Relaxed) {
                    break;
                }
                if start.elapsed() > duration {
                    let pending_transactions =
                        metrics.num_transactions_sent.load(Ordering::Relaxed)
                            - metrics.num_transactions_completed.load(Ordering::Relaxed);
                    if pending_transactions == 0 {
                        break;
                    }
                }

                metrics.report();
                std::thread::sleep(Duration::from_millis(100));
            }
            exit.store(true, Ordering::Relaxed);
        }
    }).unwrap();

    scheduler_handle.join().unwrap();
    execution_handles
        .into_iter()
        .for_each(|jh| jh.join().unwrap());
    packet_sender_handles
        .into_iter()
        .for_each(|jh| jh.join().unwrap());
}

fn start_execution_threads(
    metrics: Arc<TransactionSchedulerBenchMetrics>,
    transaction_batch_receivers: Vec<Receiver<TransactionBatchMessage>>,
    completed_transaction_sender: Sender<CompletedTransactionMessage>,
    execution_per_tx_us: u64,
    exit: Arc<AtomicBool>,
) -> Vec<JoinHandle<()>> {
    transaction_batch_receivers
        .into_iter()
        .enumerate()
        .map(|(thread_index, transaction_batch_receiver)| {
            start_execution_thread(
                metrics.clone(),
                thread_index,
                transaction_batch_receiver,
                completed_transaction_sender.clone(),
                execution_per_tx_us,
                exit.clone(),
            )
        })
        .collect()
}

fn start_execution_thread(
    metrics: Arc<TransactionSchedulerBenchMetrics>,
    thread_index: usize,
    transaction_batch_receiver: Receiver<TransactionBatchMessage>,
    completed_transaction_sender: Sender<CompletedTransactionMessage>,
    execution_per_tx_us: u64,
    exit: Arc<AtomicBool>,
) -> JoinHandle<()> {
    std::thread::Builder::new().name(format!("blockstore_processor_{}", thread_index)).spawn(move || {
        execution_worker(
            metrics,
            thread_index,
            transaction_batch_receiver,
            completed_transaction_sender,
            execution_per_tx_us,
            exit,
        )
    }).unwrap()
}

fn execution_worker(
    metrics: Arc<TransactionSchedulerBenchMetrics>,
    thread_index: usize,
    transaction_batch_receiver: Receiver<TransactionBatchMessage>,
    completed_transaction_sender: Sender<CompletedTransactionMessage>,
    execution_per_tx_us: u64,
    exit: Arc<AtomicBool>,
) {
    loop {
        if exit.load(Ordering::Relaxed) {
            break;
        }

        select! {
            recv(transaction_batch_receiver) -> maybe_tx_batch => {
                if let Ok(tx_batch) = maybe_tx_batch {
                    handle_transaction_batch(&metrics, thread_index, &completed_transaction_sender, tx_batch, execution_per_tx_us);
                }
            }
            default(Duration::from_millis(100)) => {}
        }
    }
}

fn handle_transaction_batch(
    metrics: &TransactionSchedulerBenchMetrics,
    thread_index: usize,
    completed_transaction_sender: &Sender<CompletedTransactionMessage>,
    transaction_batch: TransactionBatchMessage,
    execution_per_tx_us: u64,
) {
    let num_transactions = 1; //transaction_batch.len() as u64;
    metrics
        .num_transactions_scheduled
        .fetch_add(num_transactions as usize, Ordering::Relaxed);

    sleep(Duration::from_micros(
        num_transactions * execution_per_tx_us,
    ));

    let priority_collected = transaction_batch.task.tx.0.get_transaction_priority_details().unwrap().priority;

    metrics
        .num_transactions_completed
        .fetch_add(num_transactions as usize, Ordering::Relaxed);
    metrics
        .priority_collected
        .fetch_add(priority_collected, Ordering::Relaxed);

        completed_transaction_sender
            .send(solana_scheduler::MultiplexedPayload::FromExecute(transaction_batch))
            .unwrap();
}

const NUM_SENDERS: usize = 4;

fn spawn_packet_senders(
    metrics: Arc<TransactionSchedulerBenchMetrics>,
    high_conflict_sender: usize,
    accounts: Arc<Vec<Keypair>>,
    packet_batch_sender: Sender<BatchSenderMessage>,
    config: Arc<PacketSendingConfig>,
    duration: Duration,
    exit: Arc<AtomicBool>,
) -> Vec<JoinHandle<()>> {
    (0..NUM_SENDERS)
        .map(|i| {
            let num_accounts = if i == 0 && high_conflict_sender > 0 {
                high_conflict_sender
            } else {
                accounts.len()
            };
            spawn_packet_sender(
                metrics.clone(),
                num_accounts,
                accounts.clone(),
                packet_batch_sender.clone(),
                config.clone(),
                duration,
                exit.clone(),
            )
        })
        .collect()
}

fn spawn_packet_sender(
    metrics: Arc<TransactionSchedulerBenchMetrics>,
    num_accounts: usize,
    accounts: Arc<Vec<Keypair>>,
    packet_batch_sender: Sender<BatchSenderMessage>,
    config: Arc<PacketSendingConfig>,
    duration: Duration,
    exit: Arc<AtomicBool>,
) -> JoinHandle<()> {
    std::thread::spawn(move || {
        send_packets(
            metrics,
            num_accounts,
            accounts,
            packet_batch_sender,
            config,
            duration,
            exit,
        );
    })
}

fn send_packets(
    metrics: Arc<TransactionSchedulerBenchMetrics>,
    num_accounts: usize,
    accounts: Arc<Vec<Keypair>>,
    packet_batch_sender: Sender<BatchSenderMessage>,
    config: Arc<PacketSendingConfig>,
    duration: Duration,
    exit: Arc<AtomicBool>,
) {
    let packets_per_msg = config.packets_per_batch * config.batches_per_msg;
    let loop_frequency =
        config.packet_send_rate as f64 * packets_per_msg as f64 / NUM_SENDERS as f64;
    let loop_duration = Duration::from_secs_f64(1.0 / loop_frequency);

    info!("sending packets: packets_per_msg: {packets_per_msg} loop_frequency: {loop_frequency} loop_duration: {loop_duration:?}");

    let blockhash = Hash::default();
    let start = Instant::now();

    loop {
        if exit.load(Ordering::Relaxed) {
            break;
        }
        if start.elapsed() > duration {
            info!("stopping packet sending");
            break;
        }
        let (packet_batches, packet_build_time) = measure!(build_packet_batches(
            &config,
            num_accounts,
            &accounts,
            &blockhash
        ));
        metrics.num_transactions_sent.fetch_add(
            packet_batches.iter().map(|pb| pb.len()).sum(),
            Ordering::Relaxed,
        );
        let _ = packet_batch_sender.send(solana_scheduler::MultiplexedPayload::FromPreviousBatched(packet_batches));

        std::thread::sleep(loop_duration.saturating_sub(packet_build_time.as_duration()));
    }
}

fn build_packet_batches(
    config: &PacketSendingConfig,
    num_accounts: usize,
    accounts: &[Keypair],
    blockhash: &Hash,
) -> Vec<Vec<PreprocessedTransaction>> {
    (0..config.batches_per_msg)
        .map(|_| build_packet_batch(config, num_accounts, accounts, blockhash))
        .collect()
}

fn build_packet_batch(
    config: &PacketSendingConfig,
    num_accounts: usize,
    accounts: &[Keypair],
    blockhash: &Hash,
) -> Vec<PreprocessedTransaction> {
    (0..config.packets_per_batch)
        .map(|_| build_packet(config, num_accounts, accounts, blockhash))
        .collect()
}

fn build_packet(
    config: &PacketSendingConfig,
    num_accounts: usize,
    accounts: &[Keypair],
    blockhash: &Hash,
) -> PreprocessedTransaction {
    let get_random_account = || &accounts[rand::thread_rng().gen_range(0..num_accounts)];
    let sending_keypair = get_random_account();

    let read_account_metas = (0..config.num_read_locks_per_tx)
        .map(|_| AccountMeta::new_readonly(get_random_account().pubkey(), false));
    let write_account_metas = (0..config.num_write_locks_per_tx)
        .map(|_| AccountMeta::new(get_random_account().pubkey(), false));
    let ixs = vec![
        ComputeBudgetInstruction::set_compute_unit_price(rand::thread_rng().gen_range(50..500)),
        Instruction::new_with_bytes(
            system_program::id(),
            &[0],
            read_account_metas.chain(write_account_metas).collect(),
        ),
    ];
    let versioned_transaction = VersionedTransaction::from(Transaction::new_signed_with_payer(
        &ixs,
        Some(&sending_keypair.pubkey()),
        &[sending_keypair],
        *blockhash,
    ));

    let sanitized_tx = SanitizedTransaction::try_create(
        versioned_transaction,
        solana_sdk::transaction::MessageHash::Compute,
        None,
        solana_sdk::transaction::SimpleAddressLoader::Disabled,
        true, // require_static_program_ids
    )
    .unwrap();

    let locks = sanitized_tx.get_account_locks().unwrap();
    let writable_lock_iter = locks
        .writable
        .iter()
        .map(|address| solana_scheduler::LockAttempt::new(**address, solana_scheduler::RequestedUsage::Writable));
    let readonly_lock_iter = locks
        .readonly
        .iter()
        .map(|address| solana_scheduler::LockAttempt::new(**address, solana_scheduler::RequestedUsage::Readonly));
    let locks = writable_lock_iter.chain(readonly_lock_iter).collect::<Vec<_>>();

    Box::new((sanitized_tx, locks))
}

fn build_accounts(num_accounts: usize) -> Vec<Keypair> {
    (0..num_accounts).map(|_| Keypair::new()).collect()
}

fn build_channels<T>(num_execution_threads: usize) -> (Vec<Sender<T>>, Vec<Receiver<T>>) {
    let mut senders = Vec::with_capacity(num_execution_threads);
    let mut receivers = Vec::with_capacity(num_execution_threads);
    for _ in 0..num_execution_threads {
        let (sender, receiver) = crossbeam_channel::unbounded();
        senders.push(sender);
        receivers.push(receiver);
    }
    (senders, receivers)
}
