use {
    crate::{
        cli::{ComputeUnitPrice, Config, InstructionPaddingConfig},
        log_transaction_service::{
            create_log_transactions_service_and_sender, SignatureBatchSender, TransactionInfoBatch,
        },
        perf_utils::{sample_txs, SampleStats},
        send_batch::*,
    },
    chrono::Utc,
    log::*,
    rand::distributions::{Distribution, Uniform},
    rayon::prelude::*,
    solana_client::nonce_utils,
    solana_metrics::{self, datapoint_info},
    solana_rpc_client_api::request::MAX_MULTIPLE_ACCOUNTS,
    solana_sdk::{
        account::Account,
        clock::{DEFAULT_MS_PER_SLOT, DEFAULT_S_PER_SLOT, MAX_PROCESSING_AGE},
        compute_budget::ComputeBudgetInstruction,
        hash::Hash,
        instruction::{AccountMeta, Instruction},
        message::Message,
        native_token::Sol,
        pubkey::Pubkey,
        signature::{Keypair, Signer},
        system_instruction,
        timing::timestamp,
        transaction::Transaction,
    },
    solana_tps_client::*,
    spl_instruction_padding::instruction::wrap_instruction,
    std::{
        collections::{HashSet, VecDeque},
        process::exit,
        sync::{
            atomic::{AtomicBool, AtomicIsize, AtomicUsize, Ordering},
            Arc, RwLock,
        },
        thread::{sleep, Builder, JoinHandle},
        time::{Duration, Instant},
    },
};

// The point at which transactions become "too old", in seconds.
const MAX_TX_QUEUE_AGE: u64 = (MAX_PROCESSING_AGE as f64 * DEFAULT_S_PER_SLOT) as u64;

// Add prioritization fee to transfer transactions, if `compute_unit_price` is set.
// If `Random` the compute-unit-price is determined by generating a random number in the range
// 0..MAX_RANDOM_COMPUTE_UNIT_PRICE then multiplying by COMPUTE_UNIT_PRICE_MULTIPLIER.
// If `Fixed` the compute-unit-price is the value of the `compute-unit-price` parameter.
// It also sets transaction's compute-unit to TRANSFER_TRANSACTION_COMPUTE_UNIT. Therefore the
// max additional cost is:
// `TRANSFER_TRANSACTION_COMPUTE_UNIT * MAX_COMPUTE_UNIT_PRICE * COMPUTE_UNIT_PRICE_MULTIPLIER / 1_000_000`
const MAX_RANDOM_COMPUTE_UNIT_PRICE: u64 = 50;
const COMPUTE_UNIT_PRICE_MULTIPLIER: u64 = 1_000;
const TRANSFER_TRANSACTION_COMPUTE_UNIT: u32 = 600; // 1 transfer is plus 3 compute_budget ixs
const PADDED_TRANSFER_COMPUTE_UNIT: u32 = 3_000; // padding program execution requires consumes this amount

/// calculate maximum possible prioritization fee, if `use-randomized-compute-unit-price` is
/// enabled, round to nearest lamports.
pub fn max_lamports_for_prioritization(compute_unit_price: &Option<ComputeUnitPrice>) -> u64 {
    let Some(compute_unit_price) = compute_unit_price else {
        return 0;
    };

    let compute_unit_price = match compute_unit_price {
        ComputeUnitPrice::Random => (MAX_RANDOM_COMPUTE_UNIT_PRICE as u128)
            .saturating_mul(COMPUTE_UNIT_PRICE_MULTIPLIER as u128),
        ComputeUnitPrice::Fixed(compute_unit_price) => *compute_unit_price as u128,
    };

    const MICRO_LAMPORTS_PER_LAMPORT: u64 = 1_000_000;
    let micro_lamport_fee: u128 =
        compute_unit_price.saturating_mul(TRANSFER_TRANSACTION_COMPUTE_UNIT as u128);
    let fee = micro_lamport_fee
        .saturating_add(MICRO_LAMPORTS_PER_LAMPORT.saturating_sub(1) as u128)
        .saturating_div(MICRO_LAMPORTS_PER_LAMPORT as u128);
    u64::try_from(fee).unwrap_or(u64::MAX)
}

// In case of plain transfer transaction, set loaded account data size to 30K.
// It is large enough yet smaller than 32K page size, so it'd cost 0 extra CU.
const TRANSFER_TRANSACTION_LOADED_ACCOUNTS_DATA_SIZE: u32 = 30 * 1024;
// In case of padding program usage, we need to take into account program size
const PADDING_PROGRAM_ACCOUNT_DATA_SIZE: u32 = 28 * 1024;
fn get_transaction_loaded_accounts_data_size(enable_padding: bool) -> u32 {
    if enable_padding {
        TRANSFER_TRANSACTION_LOADED_ACCOUNTS_DATA_SIZE + PADDING_PROGRAM_ACCOUNT_DATA_SIZE
    } else {
        TRANSFER_TRANSACTION_LOADED_ACCOUNTS_DATA_SIZE
    }
}

#[derive(Debug, PartialEq, Default, Eq, Clone)]
pub(crate) struct TimestampedTransaction {
    transaction: Transaction,
    timestamp: Option<u64>,
    compute_unit_price: Option<u64>,
}

pub(crate) type SharedTransactions = Arc<RwLock<VecDeque<Vec<TimestampedTransaction>>>>;

/// Keypairs split into source and destination
/// used for transfer transactions
struct KeypairChunks<'a> {
    source: Vec<Vec<&'a Keypair>>,
    dest: Vec<VecDeque<&'a Keypair>>,
}

impl<'a> KeypairChunks<'a> {
    /// Split input slice of keypairs into two sets of chunks of given size
    fn new(keypairs: &'a [Keypair], chunk_size: usize) -> Self {
        // Use `chunk_size` as the number of conflict groups per chunk so that each destination key is unique
        Self::new_with_conflict_groups(keypairs, chunk_size, chunk_size)
    }

    /// Split input slice of keypairs into two sets of chunks of given size. Each chunk
    /// has a set of source keys and a set of destination keys. There will be
    /// `num_conflict_groups_per_chunk` unique destination keys per chunk, so that the
    /// destination keys may conflict with each other.
    fn new_with_conflict_groups(
        keypairs: &'a [Keypair],
        chunk_size: usize,
        num_conflict_groups_per_chunk: usize,
    ) -> Self {
        let mut source_keypair_chunks: Vec<Vec<&Keypair>> = Vec::new();
        let mut dest_keypair_chunks: Vec<VecDeque<&Keypair>> = Vec::new();
        for chunk in keypairs.chunks_exact(2 * chunk_size) {
            source_keypair_chunks.push(chunk[..chunk_size].iter().collect());
            dest_keypair_chunks.push(
                std::iter::repeat(&chunk[chunk_size..chunk_size + num_conflict_groups_per_chunk])
                    .flatten()
                    .take(chunk_size)
                    .collect(),
            );
        }
        KeypairChunks {
            source: source_keypair_chunks,
            dest: dest_keypair_chunks,
        }
    }
}

struct TransactionChunkGenerator<'a, 'b, T: ?Sized> {
    client: Arc<T>,
    account_chunks: KeypairChunks<'a>,
    nonce_chunks: Option<KeypairChunks<'b>>,
    chunk_index: usize,
    reclaim_lamports_back_to_source_account: bool,
    compute_unit_price: Option<ComputeUnitPrice>,
    instruction_padding_config: Option<InstructionPaddingConfig>,
    skip_tx_account_data_size: bool,
}

impl<'a, 'b, T> TransactionChunkGenerator<'a, 'b, T>
where
    T: 'static + TpsClient + Send + Sync + ?Sized,
{
    fn new(
        client: Arc<T>,
        gen_keypairs: &'a [Keypair],
        nonce_keypairs: Option<&'b Vec<Keypair>>,
        chunk_size: usize,
        compute_unit_price: Option<ComputeUnitPrice>,
        instruction_padding_config: Option<InstructionPaddingConfig>,
        num_conflict_groups: Option<usize>,
        skip_tx_account_data_size: bool,
    ) -> Self {
        let account_chunks = if let Some(num_conflict_groups) = num_conflict_groups {
            KeypairChunks::new_with_conflict_groups(gen_keypairs, chunk_size, num_conflict_groups)
        } else {
            KeypairChunks::new(gen_keypairs, chunk_size)
        };
        let nonce_chunks =
            nonce_keypairs.map(|nonce_keypairs| KeypairChunks::new(nonce_keypairs, chunk_size));

        TransactionChunkGenerator {
            client,
            account_chunks,
            nonce_chunks,
            chunk_index: 0,
            reclaim_lamports_back_to_source_account: false,
            compute_unit_price,
            instruction_padding_config,
            skip_tx_account_data_size,
        }
    }

    /// generate transactions to transfer lamports from source to destination accounts
    /// if durable nonce is used, blockhash is None
    fn generate(&mut self, blockhash: Option<&Hash>) -> Vec<TimestampedTransaction> {
        let tx_count = self.account_chunks.source.len();
        info!(
            "Signing transactions... {} (reclaim={}, blockhash={:?})",
            tx_count, self.reclaim_lamports_back_to_source_account, blockhash
        );
        let signing_start = Instant::now();

        let source_chunk = &self.account_chunks.source[self.chunk_index];
        let dest_chunk = &self.account_chunks.dest[self.chunk_index];
        let transactions = if let Some(nonce_chunks) = &self.nonce_chunks {
            let source_nonce_chunk = &nonce_chunks.source[self.chunk_index];
            let dest_nonce_chunk: &VecDeque<&Keypair> = &nonce_chunks.dest[self.chunk_index];
            generate_nonced_system_txs(
                self.client.clone(),
                source_chunk,
                dest_chunk,
                source_nonce_chunk,
                dest_nonce_chunk,
                self.reclaim_lamports_back_to_source_account,
                self.skip_tx_account_data_size,
                &self.instruction_padding_config,
            )
        } else {
            assert!(blockhash.is_some());
            generate_system_txs(
                source_chunk,
                dest_chunk,
                self.reclaim_lamports_back_to_source_account,
                blockhash.unwrap(),
                &self.instruction_padding_config,
                &self.compute_unit_price,
                self.skip_tx_account_data_size,
            )
        };

        let duration = signing_start.elapsed();
        let ns = duration.as_secs() * 1_000_000_000 + u64::from(duration.subsec_nanos());
        let bsps = (tx_count) as f64 / ns as f64;
        let nsps = ns as f64 / (tx_count) as f64;
        info!(
            "Done. {:.2} thousand signatures per second, {:.2} us per signature, {} ms total time, {:?}",
            bsps * 1_000_000_f64,
            nsps / 1_000_f64,
            duration.as_millis(),
            blockhash,
        );
        datapoint_info!(
            "bench-tps-generate_txs",
            ("duration", duration.as_micros() as i64, i64)
        );

        transactions
    }

    fn advance(&mut self) {
        // Rotate destination keypairs so that the next round of transactions will have different
        // transaction signatures even when blockhash is reused.
        self.account_chunks.dest[self.chunk_index].rotate_left(1);
        if let Some(nonce_chunks) = &mut self.nonce_chunks {
            nonce_chunks.dest[self.chunk_index].rotate_left(1);
        }
        // Move on to next chunk
        self.chunk_index = (self.chunk_index + 1) % self.account_chunks.source.len();

        // Switch directions after transferring for each "chunk"
        if self.chunk_index == 0 {
            self.reclaim_lamports_back_to_source_account =
                !self.reclaim_lamports_back_to_source_account;
        }
    }
}

fn wait_for_target_slots_per_epoch<T>(target_slots_per_epoch: u64, client: &Arc<T>)
where
    T: 'static + TpsClient + Send + Sync + ?Sized,
{
    if target_slots_per_epoch != 0 {
        info!(
            "Waiting until epochs are {} slots long..",
            target_slots_per_epoch
        );
        loop {
            if let Ok(epoch_info) = client.get_epoch_info() {
                if epoch_info.slots_in_epoch >= target_slots_per_epoch {
                    info!("Done epoch_info: {:?}", epoch_info);
                    break;
                }
                info!(
                    "Waiting for epoch: {} now: {}",
                    target_slots_per_epoch, epoch_info.slots_in_epoch
                );
            }
            sleep(Duration::from_secs(3));
        }
    }
}

fn create_sampler_thread<T>(
    client: &Arc<T>,
    exit_signal: Arc<AtomicBool>,
    sample_period: u64,
    maxes: &Arc<RwLock<Vec<(String, SampleStats)>>>,
) -> JoinHandle<()>
where
    T: 'static + TpsClient + Send + Sync + ?Sized,
{
    info!("Sampling TPS every {} second...", sample_period);
    let maxes = maxes.clone();
    let client = client.clone();
    Builder::new()
        .name("solana-client-sample".to_string())
        .spawn(move || {
            sample_txs(exit_signal, &maxes, sample_period, &client);
        })
        .unwrap()
}

fn generate_chunked_transfers<T: 'static + TpsClient + Send + Sync + ?Sized>(
    recent_blockhash: Arc<RwLock<Hash>>,
    shared_txs: &SharedTransactions,
    shared_tx_active_thread_count: Arc<AtomicIsize>,
    mut chunk_generator: TransactionChunkGenerator<'_, '_, T>,
    threads: usize,
    duration: Duration,
    sustained: bool,
    use_durable_nonce: bool,
) {
    // generate and send transactions for the specified duration
    let start = Instant::now();
    let mut last_generate_txs_time = Instant::now();

    while start.elapsed() < duration {
        generate_txs(
            shared_txs,
            &recent_blockhash,
            &mut chunk_generator,
            threads,
            use_durable_nonce,
        );

        datapoint_info!(
            "blockhash_stats",
            (
                "time_elapsed_since_last_generate_txs",
                last_generate_txs_time.elapsed().as_millis(),
                i64
            )
        );

        last_generate_txs_time = Instant::now();

        // In sustained mode, overlap the transfers with generation. This has higher average
        // performance but lower peak performance in tested environments.
        if sustained {
            // Ensure that we don't generate more transactions than we can handle.
            while shared_txs.read().unwrap().len() > 2 * threads {
                sleep(Duration::from_millis(1));
            }
        } else {
            while !shared_txs.read().unwrap().is_empty()
                || shared_tx_active_thread_count.load(Ordering::Relaxed) > 0
            {
                sleep(Duration::from_millis(1));
            }
        }
        chunk_generator.advance();
    }
}

fn create_sender_threads<T>(
    client: &Arc<T>,
    shared_txs: &SharedTransactions,
    thread_batch_sleep_ms: usize,
    total_tx_sent_count: &Arc<AtomicUsize>,
    threads: usize,
    exit_signal: Arc<AtomicBool>,
    shared_tx_active_thread_count: &Arc<AtomicIsize>,
    signatures_sender: Option<SignatureBatchSender>,
) -> Vec<JoinHandle<()>>
where
    T: 'static + TpsClient + Send + Sync + ?Sized,
{
    (0..threads)
        .map(|_| {
            let exit_signal = exit_signal.clone();
            let shared_txs = shared_txs.clone();
            let shared_tx_active_thread_count = shared_tx_active_thread_count.clone();
            let total_tx_sent_count = total_tx_sent_count.clone();
            let client = client.clone();
            let signatures_sender = signatures_sender.clone();
            Builder::new()
                .name("solana-client-sender".to_string())
                .spawn(move || {
                    do_tx_transfers(
                        &exit_signal,
                        &shared_txs,
                        &shared_tx_active_thread_count,
                        &total_tx_sent_count,
                        thread_batch_sleep_ms,
                        &client,
                        signatures_sender,
                    );
                })
                .unwrap()
        })
        .collect()
}

pub fn do_bench_tps<T>(
    client: Arc<T>,
    config: Config,
    gen_keypairs: Vec<Keypair>,
    nonce_keypairs: Option<Vec<Keypair>>,
) -> u64
where
    T: 'static + TpsClient + Send + Sync + ?Sized,
{
    let Config {
        id,
        threads,
        thread_batch_sleep_ms,
        duration,
        tx_count,
        sustained,
        target_slots_per_epoch,
        compute_unit_price,
        skip_tx_account_data_size,
        use_durable_nonce,
        instruction_padding_config,
        num_conflict_groups,
        block_data_file,
        transaction_data_file,
        ..
    } = config;

    assert!(gen_keypairs.len() >= 2 * tx_count);
    let chunk_generator = TransactionChunkGenerator::new(
        client.clone(),
        &gen_keypairs,
        nonce_keypairs.as_ref(),
        tx_count,
        compute_unit_price,
        instruction_padding_config,
        num_conflict_groups,
        skip_tx_account_data_size,
    );

    let first_tx_count = loop {
        match client.get_transaction_count() {
            Ok(count) => break count,
            Err(err) => {
                info!("Couldn't get transaction count: {:?}", err);
                sleep(Duration::from_secs(1));
            }
        }
    };
    info!("Initial transaction count {}", first_tx_count);

    let exit_signal = Arc::new(AtomicBool::new(false));

    // Setup a thread per validator to sample every period
    // collect the max transaction rate and total tx count seen
    let maxes = Arc::new(RwLock::new(Vec::new()));
    let sample_period = 1; // in seconds
    let sample_thread = create_sampler_thread(&client, exit_signal.clone(), sample_period, &maxes);

    let shared_txs: SharedTransactions = Arc::new(RwLock::new(VecDeque::new()));

    let blockhash = Arc::new(RwLock::new(get_latest_blockhash(client.as_ref())));
    let shared_tx_active_thread_count = Arc::new(AtomicIsize::new(0));
    let total_tx_sent_count = Arc::new(AtomicUsize::new(0));

    // if we use durable nonce, we don't need blockhash thread
    let blockhash_thread = if !use_durable_nonce {
        let exit_signal = exit_signal.clone();
        let blockhash = blockhash.clone();
        let client = client.clone();
        let id = id.pubkey();
        Some(
            Builder::new()
                .name("solana-blockhash-poller".to_string())
                .spawn(move || {
                    poll_blockhash(&exit_signal, &blockhash, &client, &id);
                })
                .unwrap(),
        )
    } else {
        None
    };

    let (log_transaction_service, signatures_sender) = create_log_transactions_service_and_sender(
        &client,
        block_data_file.as_deref(),
        transaction_data_file.as_deref(),
    );

    let sender_threads = create_sender_threads(
        &client,
        &shared_txs,
        thread_batch_sleep_ms,
        &total_tx_sent_count,
        threads,
        exit_signal.clone(),
        &shared_tx_active_thread_count,
        signatures_sender,
    );

    wait_for_target_slots_per_epoch(target_slots_per_epoch, &client);

    let start = Instant::now();

    generate_chunked_transfers(
        blockhash,
        &shared_txs,
        shared_tx_active_thread_count,
        chunk_generator,
        threads,
        duration,
        sustained,
        use_durable_nonce,
    );

    // Stop the sampling threads so it will collect the stats
    exit_signal.store(true, Ordering::Relaxed);

    info!("Waiting for sampler threads...");
    if let Err(err) = sample_thread.join() {
        info!("  join() failed with: {:?}", err);
    }

    // join the tx send threads
    info!("Waiting for transmit threads...");
    for t in sender_threads {
        if let Err(err) = t.join() {
            info!("  join() failed with: {:?}", err);
        }
    }

    if let Some(blockhash_thread) = blockhash_thread {
        info!("Waiting for blockhash thread...");
        if let Err(err) = blockhash_thread.join() {
            info!("  join() failed with: {:?}", err);
        }
    }

    if let Some(log_transaction_service) = log_transaction_service {
        info!("Waiting for log_transaction_service thread...");
        if let Err(err) = log_transaction_service.join() {
            info!("  join() failed with: {:?}", err);
        }
    }

    if let Some(nonce_keypairs) = nonce_keypairs {
        withdraw_durable_nonce_accounts(client.clone(), &gen_keypairs, &nonce_keypairs);
    }

    let balance = client.get_balance(&id.pubkey()).unwrap_or(0);
    metrics_submit_lamport_balance(balance);

    compute_and_report_stats(
        &maxes,
        sample_period,
        &start.elapsed(),
        total_tx_sent_count.load(Ordering::Relaxed),
    );

    let r_maxes = maxes.read().unwrap();
    r_maxes.first().unwrap().1.txs
}

fn metrics_submit_lamport_balance(lamport_balance: u64) {
    info!("Token balance: {}", lamport_balance);
    datapoint_info!(
        "bench-tps-lamport_balance",
        ("balance", lamport_balance, i64)
    );
}

fn generate_system_txs(
    source: &[&Keypair],
    dest: &VecDeque<&Keypair>,
    reclaim: bool,
    blockhash: &Hash,
    instruction_padding_config: &Option<InstructionPaddingConfig>,
    compute_unit_price: &Option<ComputeUnitPrice>,
    skip_tx_account_data_size: bool,
) -> Vec<TimestampedTransaction> {
    let pairs: Vec<_> = if !reclaim {
        source.iter().zip(dest.iter()).collect()
    } else {
        dest.iter().zip(source.iter()).collect()
    };

    if let Some(compute_unit_price) = compute_unit_price {
        let compute_unit_prices = match compute_unit_price {
            ComputeUnitPrice::Random => {
                let mut rng = rand::thread_rng();
                let range = Uniform::from(0..MAX_RANDOM_COMPUTE_UNIT_PRICE);
                (0..pairs.len())
                    .map(|_| {
                        range
                            .sample(&mut rng)
                            .saturating_mul(COMPUTE_UNIT_PRICE_MULTIPLIER)
                    })
                    .collect()
            }
            ComputeUnitPrice::Fixed(compute_unit_price) => vec![*compute_unit_price; pairs.len()],
        };

        let pairs_with_compute_unit_prices: Vec<_> =
            pairs.iter().zip(compute_unit_prices.iter()).collect();

        pairs_with_compute_unit_prices
            .par_iter()
            .map(|((from, to), compute_unit_price)| {
                let compute_unit_price = Some(**compute_unit_price);
                TimestampedTransaction {
                    transaction: transfer_with_compute_unit_price_and_padding(
                        from,
                        &to.pubkey(),
                        1,
                        *blockhash,
                        instruction_padding_config,
                        compute_unit_price,
                        skip_tx_account_data_size,
                    ),
                    timestamp: Some(timestamp()),
                    compute_unit_price,
                }
            })
            .collect()
    } else {
        pairs
            .par_iter()
            .map(|(from, to)| TimestampedTransaction {
                transaction: transfer_with_compute_unit_price_and_padding(
                    from,
                    &to.pubkey(),
                    1,
                    *blockhash,
                    instruction_padding_config,
                    None,
                    skip_tx_account_data_size,
                ),
                timestamp: Some(timestamp()),
                compute_unit_price: None,
            })
            .collect()
    }
}

fn transfer_with_compute_unit_price_and_padding(
    from_keypair: &Keypair,
    to: &Pubkey,
    lamports: u64,
    recent_blockhash: Hash,
    instruction_padding_config: &Option<InstructionPaddingConfig>,
    compute_unit_price: Option<u64>,
    skip_tx_account_data_size: bool,
) -> Transaction {
    let from_pubkey = from_keypair.pubkey();
    let transfer_instruction = system_instruction::transfer(&from_pubkey, to, lamports);
    let instruction = if let Some(instruction_padding_config) = instruction_padding_config {
        wrap_instruction(
            instruction_padding_config.program_id,
            transfer_instruction,
            vec![],
            instruction_padding_config.data_size,
        )
        .expect("Could not create padded instruction")
    } else {
        transfer_instruction
    };
    let mut instructions = vec![];
    if !skip_tx_account_data_size {
        instructions.push(
            ComputeBudgetInstruction::set_loaded_accounts_data_size_limit(
                get_transaction_loaded_accounts_data_size(instruction_padding_config.is_some()),
            ),
        )
    }
    instructions.push(instruction);
    if instruction_padding_config.is_some() {
        // By default, CU budget is DEFAULT_INSTRUCTION_COMPUTE_UNIT_LIMIT which is much larger than needed
        instructions.push(ComputeBudgetInstruction::set_compute_unit_limit(
            PADDED_TRANSFER_COMPUTE_UNIT,
        ));
    }

    if let Some(compute_unit_price) = compute_unit_price {
        instructions.extend_from_slice(&[
            ComputeBudgetInstruction::set_compute_unit_limit(TRANSFER_TRANSACTION_COMPUTE_UNIT),
            ComputeBudgetInstruction::set_compute_unit_price(compute_unit_price),
        ])
    }
    let message = Message::new(&instructions, Some(&from_pubkey));
    Transaction::new(&[from_keypair], message, recent_blockhash)
}

fn get_nonce_accounts<T: 'static + TpsClient + Send + Sync + ?Sized>(
    client: &Arc<T>,
    nonce_pubkeys: &[Pubkey],
) -> Vec<Option<Account>> {
    // get_multiple_accounts supports maximum MAX_MULTIPLE_ACCOUNTS pubkeys in request
    assert!(nonce_pubkeys.len() <= MAX_MULTIPLE_ACCOUNTS);
    loop {
        match client.get_multiple_accounts(nonce_pubkeys) {
            Ok(nonce_accounts) => {
                return nonce_accounts;
            }
            Err(err) => {
                info!("Couldn't get durable nonce account: {:?}", err);
                sleep(Duration::from_secs(1));
            }
        }
    }
}

fn get_nonce_blockhashes<T: 'static + TpsClient + Send + Sync + ?Sized>(
    client: &Arc<T>,
    nonce_pubkeys: &[Pubkey],
) -> Vec<Hash> {
    let num_accounts = nonce_pubkeys.len();
    let mut blockhashes = vec![Hash::default(); num_accounts];
    let mut unprocessed = (0..num_accounts).collect::<HashSet<_>>();

    let mut request_pubkeys = Vec::<Pubkey>::with_capacity(num_accounts);
    let mut request_indexes = Vec::<usize>::with_capacity(num_accounts);

    while !unprocessed.is_empty() {
        for i in &unprocessed {
            request_pubkeys.push(nonce_pubkeys[*i]);
            request_indexes.push(*i);
        }

        let num_unprocessed_before = unprocessed.len();
        let accounts: Vec<Option<Account>> = nonce_pubkeys
            .chunks(MAX_MULTIPLE_ACCOUNTS)
            .flat_map(|pubkeys| get_nonce_accounts(client, pubkeys))
            .collect();

        for (account, index) in accounts.iter().zip(request_indexes.iter()) {
            if let Some(nonce_account) = account {
                let nonce_data = nonce_utils::data_from_account(nonce_account).unwrap();
                blockhashes[*index] = nonce_data.blockhash();
                unprocessed.remove(index);
            }
        }
        let num_unprocessed_after = unprocessed.len();
        debug!(
            "Received {} durable nonce accounts",
            num_unprocessed_before - num_unprocessed_after
        );
        request_pubkeys.clear();
        request_indexes.clear();
    }
    blockhashes
}

fn nonced_transfer_with_padding(
    from_keypair: &Keypair,
    to: &Pubkey,
    lamports: u64,
    nonce_account: &Pubkey,
    nonce_authority: &Keypair,
    nonce_hash: Hash,
    skip_tx_account_data_size: bool,
    instruction_padding_config: &Option<InstructionPaddingConfig>,
) -> Transaction {
    let from_pubkey = from_keypair.pubkey();
    let transfer_instruction = system_instruction::transfer(&from_pubkey, to, lamports);
    let instruction = if let Some(instruction_padding_config) = instruction_padding_config {
        wrap_instruction(
            instruction_padding_config.program_id,
            transfer_instruction,
            vec![],
            instruction_padding_config.data_size,
        )
        .expect("Could not create padded instruction")
    } else {
        transfer_instruction
    };
    let mut instructions = vec![];
    if !skip_tx_account_data_size {
        instructions.push(
            ComputeBudgetInstruction::set_loaded_accounts_data_size_limit(
                get_transaction_loaded_accounts_data_size(instruction_padding_config.is_some()),
            ),
        )
    }
    instructions.push(instruction);
    let message = Message::new_with_nonce(
        instructions,
        Some(&from_pubkey),
        nonce_account,
        &nonce_authority.pubkey(),
    );
    Transaction::new(&[from_keypair, nonce_authority], message, nonce_hash)
}

fn generate_nonced_system_txs<T: 'static + TpsClient + Send + Sync + ?Sized>(
    client: Arc<T>,
    source: &[&Keypair],
    dest: &VecDeque<&Keypair>,
    source_nonce: &[&Keypair],
    dest_nonce: &VecDeque<&Keypair>,
    reclaim: bool,
    skip_tx_account_data_size: bool,
    instruction_padding_config: &Option<InstructionPaddingConfig>,
) -> Vec<TimestampedTransaction> {
    let length = source.len();
    let mut transactions: Vec<TimestampedTransaction> = Vec::with_capacity(length);
    if !reclaim {
        let pubkeys: Vec<Pubkey> = source_nonce
            .iter()
            .map(|keypair| keypair.pubkey())
            .collect();

        let blockhashes: Vec<Hash> = get_nonce_blockhashes(&client, &pubkeys);
        for i in 0..length {
            transactions.push(TimestampedTransaction {
                transaction: nonced_transfer_with_padding(
                    source[i],
                    &dest[i].pubkey(),
                    1,
                    &source_nonce[i].pubkey(),
                    source[i],
                    blockhashes[i],
                    skip_tx_account_data_size,
                    instruction_padding_config,
                ),
                timestamp: None,
                compute_unit_price: None,
            });
        }
    } else {
        let pubkeys: Vec<Pubkey> = dest_nonce.iter().map(|keypair| keypair.pubkey()).collect();
        let blockhashes: Vec<Hash> = get_nonce_blockhashes(&client, &pubkeys);

        for i in 0..length {
            transactions.push(TimestampedTransaction {
                transaction: nonced_transfer_with_padding(
                    dest[i],
                    &source[i].pubkey(),
                    1,
                    &dest_nonce[i].pubkey(),
                    dest[i],
                    blockhashes[i],
                    skip_tx_account_data_size,
                    instruction_padding_config,
                ),
                timestamp: None,
                compute_unit_price: None,
            });
        }
    }
    transactions
}

fn generate_txs<T: 'static + TpsClient + Send + Sync + ?Sized>(
    shared_txs: &SharedTransactions,
    blockhash: &Arc<RwLock<Hash>>,
    chunk_generator: &mut TransactionChunkGenerator<'_, '_, T>,
    threads: usize,
    use_durable_nonce: bool,
) {
    let transactions = if use_durable_nonce {
        chunk_generator.generate(None)
    } else {
        let blockhash = blockhash.read().map(|x| *x).ok();
        chunk_generator.generate(blockhash.as_ref())
    };

    let sz = transactions.len() / threads;
    let chunks: Vec<_> = transactions.chunks(sz).collect();
    {
        let mut shared_txs_wl = shared_txs.write().unwrap();
        for chunk in chunks {
            shared_txs_wl.push_back(chunk.to_vec());
        }
    }
}

fn get_new_latest_blockhash<T: TpsClient + ?Sized>(
    client: &Arc<T>,
    blockhash: &Hash,
) -> Option<Hash> {
    let start = Instant::now();
    while start.elapsed().as_secs() < 5 {
        if let Ok(new_blockhash) = client.get_latest_blockhash() {
            if new_blockhash != *blockhash {
                return Some(new_blockhash);
            }
        }
        debug!("Got same blockhash ({:?}), will retry...", blockhash);

        // Retry ~twice during a slot
        sleep(Duration::from_millis(DEFAULT_MS_PER_SLOT / 2));
    }
    None
}

fn poll_blockhash<T: TpsClient + ?Sized>(
    exit_signal: &AtomicBool,
    blockhash: &Arc<RwLock<Hash>>,
    client: &Arc<T>,
    id: &Pubkey,
) {
    let mut blockhash_last_updated = Instant::now();
    let mut last_error_log = Instant::now();
    loop {
        let blockhash_updated = {
            let old_blockhash = *blockhash.read().unwrap();
            if let Some(new_blockhash) = get_new_latest_blockhash(client, &old_blockhash) {
                *blockhash.write().unwrap() = new_blockhash;
                blockhash_last_updated = Instant::now();
                true
            } else {
                if blockhash_last_updated.elapsed().as_secs() > 120 {
                    eprintln!("Blockhash is stuck");
                    exit(1)
                } else if blockhash_last_updated.elapsed().as_secs() > 30
                    && last_error_log.elapsed().as_secs() >= 1
                {
                    last_error_log = Instant::now();
                    error!("Blockhash is not updating");
                }
                false
            }
        };

        if blockhash_updated {
            let balance = client.get_balance(id).unwrap_or(0);
            metrics_submit_lamport_balance(balance);
            datapoint_info!(
                "blockhash_stats",
                (
                    "time_elapsed_since_last_blockhash_update",
                    blockhash_last_updated.elapsed().as_millis(),
                    i64
                )
            )
        }

        if exit_signal.load(Ordering::Relaxed) {
            break;
        }

        sleep(Duration::from_millis(50));
    }
}

fn do_tx_transfers<T: TpsClient + ?Sized>(
    exit_signal: &AtomicBool,
    shared_txs: &SharedTransactions,
    shared_tx_thread_count: &Arc<AtomicIsize>,
    total_tx_sent_count: &Arc<AtomicUsize>,
    thread_batch_sleep_ms: usize,
    client: &Arc<T>,
    signatures_sender: Option<SignatureBatchSender>,
) {
    let mut last_sent_time = timestamp();
    'thread_loop: loop {
        if thread_batch_sleep_ms > 0 {
            sleep(Duration::from_millis(thread_batch_sleep_ms as u64));
        }
        let txs = {
            let mut shared_txs_wl = shared_txs.write().expect("write lock in do_tx_transfers");
            shared_txs_wl.pop_front()
        };
        if let Some(txs) = txs {
            shared_tx_thread_count.fetch_add(1, Ordering::Relaxed);
            let num_txs = txs.len();
            info!("Transferring 1 unit {} times...", num_txs);
            let transfer_start = Instant::now();
            let mut old_transactions = false;
            let mut min_timestamp = u64::MAX;
            let mut transactions = Vec::<_>::with_capacity(num_txs);
            let mut signatures = Vec::<_>::with_capacity(num_txs);
            let mut compute_unit_prices = Vec::<_>::with_capacity(num_txs);
            for tx in txs {
                let now = timestamp();
                // Transactions without durable nonce that are too old will be rejected by the cluster Don't bother
                // sending them.
                if let Some(tx_timestamp) = tx.timestamp {
                    if tx_timestamp < min_timestamp {
                        min_timestamp = tx_timestamp;
                    }
                    if now > tx_timestamp && now - tx_timestamp > 1000 * MAX_TX_QUEUE_AGE {
                        old_transactions = true;
                        continue;
                    }
                }
                signatures.push(tx.transaction.signatures[0]);
                transactions.push(tx.transaction);
                compute_unit_prices.push(tx.compute_unit_price);
            }

            if min_timestamp != u64::MAX {
                datapoint_info!(
                    "bench-tps-do_tx_transfers",
                    ("oldest-blockhash-age", timestamp() - min_timestamp, i64),
                );
            }

            if let Some(signatures_sender) = &signatures_sender {
                if let Err(error) = signatures_sender.send(TransactionInfoBatch {
                    signatures,
                    sent_at: Utc::now(),
                    compute_unit_prices,
                }) {
                    error!("Receiver has been dropped with error `{error}`, stop sending transactions.");
                    break 'thread_loop;
                }
            }

            if let Err(error) = client.send_batch(transactions) {
                warn!("send_batch_sync in do_tx_transfers failed: {}", error);
            }

            datapoint_info!(
                "bench-tps-do_tx_transfers",
                (
                    "time-elapsed-since-last-send",
                    timestamp() - last_sent_time,
                    i64
                ),
            );

            last_sent_time = timestamp();

            if old_transactions {
                let mut shared_txs_wl = shared_txs.write().expect("write lock in do_tx_transfers");
                shared_txs_wl.clear();
            }
            shared_tx_thread_count.fetch_add(-1, Ordering::Relaxed);
            total_tx_sent_count.fetch_add(num_txs, Ordering::Relaxed);
            info!(
                "Tx send done. {} ms {} tps",
                transfer_start.elapsed().as_millis(),
                num_txs as f32 / transfer_start.elapsed().as_secs_f32(),
            );
            datapoint_info!(
                "bench-tps-do_tx_transfers",
                ("duration", transfer_start.elapsed().as_micros() as i64, i64),
                ("count", num_txs, i64)
            );
        }
        if exit_signal.load(Ordering::Relaxed) {
            break;
        }
    }
}

fn compute_and_report_stats(
    maxes: &Arc<RwLock<Vec<(String, SampleStats)>>>,
    sample_period: u64,
    tx_send_elapsed: &Duration,
    total_tx_send_count: usize,
) {
    // Compute/report stats
    let mut max_of_maxes = 0.0;
    let mut max_tx_count = 0;
    let mut nodes_with_zero_tps = 0;
    let mut total_maxes = 0.0;
    info!(" Node address        |       Max TPS | Total Transactions");
    info!("---------------------+---------------+--------------------");

    for (sock, stats) in maxes.read().unwrap().iter() {
        let maybe_flag = match stats.txs {
            0 => "!!!!!",
            _ => "",
        };

        info!(
            "{:20} | {:13.2} | {} {}",
            sock, stats.tps, stats.txs, maybe_flag
        );

        if stats.tps == 0.0 {
            nodes_with_zero_tps += 1;
        }
        total_maxes += stats.tps;

        if stats.tps > max_of_maxes {
            max_of_maxes = stats.tps;
        }
        if stats.txs > max_tx_count {
            max_tx_count = stats.txs;
        }
    }

    if total_maxes > 0.0 {
        let num_nodes_with_tps = maxes.read().unwrap().len() - nodes_with_zero_tps;
        let average_max = total_maxes / num_nodes_with_tps as f32;
        info!(
            "\nAverage max TPS: {:.2}, {} nodes had 0 TPS",
            average_max, nodes_with_zero_tps
        );
    }

    let total_tx_send_count = total_tx_send_count as u64;
    let drop_rate = if total_tx_send_count > max_tx_count {
        (total_tx_send_count - max_tx_count) as f64 / total_tx_send_count as f64
    } else {
        0.0
    };
    info!(
        "\nHighest TPS: {:.2} sampling period {}s max transactions: {} clients: {} drop rate: {:.2}",
        max_of_maxes,
        sample_period,
        max_tx_count,
        maxes.read().unwrap().len(),
        drop_rate,
    );
    info!(
        "\tAverage TPS: {}",
        max_tx_count as f32 / tx_send_elapsed.as_secs_f32()
    );
}

pub fn generate_and_fund_keypairs<T: 'static + TpsClient + Send + Sync + ?Sized>(
    client: Arc<T>,
    funding_key: &Keypair,
    keypair_count: usize,
    lamports_per_account: u64,
    skip_tx_account_data_size: bool,
    enable_padding: bool,
) -> TpsClientResult<Vec<Keypair>> {
    let rent = client.get_minimum_balance_for_rent_exemption(0)?;
    let lamports_per_account = lamports_per_account + rent;

    info!("Creating {} keypairs...", keypair_count);
    let (mut keypairs, extra) = generate_keypairs(funding_key, keypair_count as u64);
    fund_keypairs(
        client,
        funding_key,
        &keypairs,
        extra,
        lamports_per_account,
        skip_tx_account_data_size,
        enable_padding,
    )?;

    // 'generate_keypairs' generates extra keys to be able to have size-aligned funding batches for fund_keys.
    keypairs.truncate(keypair_count);

    Ok(keypairs)
}

pub fn fund_keypairs<T: 'static + TpsClient + Send + Sync + ?Sized>(
    client: Arc<T>,
    funding_key: &Keypair,
    keypairs: &[Keypair],
    extra: u64,
    lamports_per_account: u64,
    skip_tx_account_data_size: bool,
    enable_padding: bool,
) -> TpsClientResult<()> {
    let rent = client.get_minimum_balance_for_rent_exemption(0)?;
    info!("Get lamports...");

    // Sample the first keypair, to prevent lamport loss on repeated solana-bench-tps executions
    let first_key = keypairs[0].pubkey();
    let first_keypair_balance = client.get_balance(&first_key).unwrap_or(0);

    // Sample the last keypair, to check if funding was already completed
    let last_key = keypairs[keypairs.len() - 1].pubkey();
    let last_keypair_balance = client.get_balance(&last_key).unwrap_or(0);

    // Repeated runs will eat up keypair balances from transaction fees. In order to quickly
    //   start another bench-tps run without re-funding all of the keypairs, check if the
    //   keypairs still have at least 80% of the expected funds. That should be enough to
    //   pay for the transaction fees in a new run.
    let enough_lamports = 8 * lamports_per_account / 10;
    if first_keypair_balance < enough_lamports || last_keypair_balance < enough_lamports {
        let single_sig_message = Message::new_with_blockhash(
            &[Instruction::new_with_bytes(
                Pubkey::new_unique(),
                &[],
                vec![AccountMeta::new(Pubkey::new_unique(), true)],
            )],
            None,
            &client.get_latest_blockhash().unwrap(),
        );
        let max_fee = client.get_fee_for_message(&single_sig_message).unwrap();
        let extra_fees = extra * max_fee;
        let total_keypairs = keypairs.len() as u64 + 1; // Add one for funding keypair
        let total = lamports_per_account * total_keypairs + extra_fees;

        let funding_key_balance = client.get_balance(&funding_key.pubkey()).unwrap_or(0);
        info!(
            "Funding keypair balance: {} max_fee: {} lamports_per_account: {} extra: {} total: {}",
            funding_key_balance, max_fee, lamports_per_account, extra, total
        );

        if funding_key_balance < total + rent {
            error!(
                "funder has {}, needed {}",
                Sol(funding_key_balance),
                Sol(total)
            );
            let latest_blockhash = get_latest_blockhash(client.as_ref());
            if client
                .request_airdrop_with_blockhash(
                    &funding_key.pubkey(),
                    total + rent - funding_key_balance,
                    &latest_blockhash,
                )
                .is_err()
            {
                return Err(TpsClientError::AirdropFailure);
            }
        }
        let data_size_limit = (!skip_tx_account_data_size)
            .then(|| get_transaction_loaded_accounts_data_size(enable_padding));

        fund_keys(
            client,
            funding_key,
            keypairs,
            total,
            max_fee,
            lamports_per_account,
            data_size_limit,
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        solana_feature_set::FeatureSet,
        solana_runtime::{bank::Bank, bank_client::BankClient, bank_forks::BankForks},
        solana_sdk::{
            commitment_config::CommitmentConfig,
            fee_calculator::FeeRateGovernor,
            genesis_config::{create_genesis_config, GenesisConfig},
            native_token::sol_to_lamports,
            nonce::State,
        },
    };

    fn bank_with_all_features(
        genesis_config: &GenesisConfig,
    ) -> (Arc<Bank>, Arc<RwLock<BankForks>>) {
        let mut bank = Bank::new_for_tests(genesis_config);
        bank.feature_set = Arc::new(FeatureSet::all_enabled());
        bank.wrap_with_bank_forks_for_tests()
    }

    #[test]
    fn test_bench_tps_bank_client() {
        let (genesis_config, id) = create_genesis_config(sol_to_lamports(10_000.0));
        let (bank, _bank_forks) = bank_with_all_features(&genesis_config);
        let client = Arc::new(BankClient::new_shared(bank));

        let config = Config {
            id,
            tx_count: 10,
            duration: Duration::from_secs(5),
            ..Config::default()
        };

        let keypair_count = config.tx_count * config.keypair_multiplier;
        let keypairs =
            generate_and_fund_keypairs(client.clone(), &config.id, keypair_count, 20, false, false)
                .unwrap();

        do_bench_tps(client, config, keypairs, None);
    }

    #[test]
    fn test_bench_tps_fund_keys() {
        let (genesis_config, id) = create_genesis_config(sol_to_lamports(10_000.0));
        let (bank, _bank_forks) = bank_with_all_features(&genesis_config);
        let client = Arc::new(BankClient::new_shared(bank));
        let keypair_count = 20;
        let lamports = 20;
        let rent = client.get_minimum_balance_for_rent_exemption(0).unwrap();

        let keypairs =
            generate_and_fund_keypairs(client.clone(), &id, keypair_count, lamports, false, false)
                .unwrap();

        for kp in &keypairs {
            assert_eq!(
                client
                    .get_balance_with_commitment(&kp.pubkey(), CommitmentConfig::processed())
                    .unwrap(),
                lamports + rent
            );
        }
    }

    #[test]
    fn test_bench_tps_fund_keys_with_fees() {
        let (mut genesis_config, id) = create_genesis_config(sol_to_lamports(10_000.0));
        let fee_rate_governor = FeeRateGovernor::new(11, 0);
        genesis_config.fee_rate_governor = fee_rate_governor;
        let (bank, _bank_forks) = bank_with_all_features(&genesis_config);
        let client = Arc::new(BankClient::new_shared(bank));
        let keypair_count = 20;
        let lamports = 20;
        let rent = client.get_minimum_balance_for_rent_exemption(0).unwrap();

        let keypairs =
            generate_and_fund_keypairs(client.clone(), &id, keypair_count, lamports, false, false)
                .unwrap();

        for kp in &keypairs {
            assert_eq!(client.get_balance(&kp.pubkey()).unwrap(), lamports + rent);
        }
    }

    #[test]
    fn test_bench_tps_create_durable_nonce() {
        let (genesis_config, id) = create_genesis_config(sol_to_lamports(10_000.0));
        let (bank, _bank_forks) = bank_with_all_features(&genesis_config);
        let client = Arc::new(BankClient::new_shared(bank));
        let keypair_count = 10;
        let lamports = 10_000_000;

        let authority_keypairs =
            generate_and_fund_keypairs(client.clone(), &id, keypair_count, lamports, false, false)
                .unwrap();

        let nonce_keypairs = generate_durable_nonce_accounts(client.clone(), &authority_keypairs);

        let rent = client
            .get_minimum_balance_for_rent_exemption(State::size())
            .unwrap();
        for kp in &nonce_keypairs {
            assert_eq!(
                client
                    .get_balance_with_commitment(&kp.pubkey(), CommitmentConfig::processed())
                    .unwrap(),
                rent
            );
        }
        withdraw_durable_nonce_accounts(client, &authority_keypairs, &nonce_keypairs)
    }

    #[test]
    fn test_bench_tps_key_chunks_new() {
        let num_keypairs = 16;
        let chunk_size = 4;
        let keypairs = std::iter::repeat_with(Keypair::new)
            .take(num_keypairs)
            .collect::<Vec<_>>();

        let chunks = KeypairChunks::new(&keypairs, chunk_size);
        assert_eq!(
            chunks.source[0],
            &[&keypairs[0], &keypairs[1], &keypairs[2], &keypairs[3]]
        );
        assert_eq!(
            chunks.dest[0],
            &[&keypairs[4], &keypairs[5], &keypairs[6], &keypairs[7]]
        );
        assert_eq!(
            chunks.source[1],
            &[&keypairs[8], &keypairs[9], &keypairs[10], &keypairs[11]]
        );
        assert_eq!(
            chunks.dest[1],
            &[&keypairs[12], &keypairs[13], &keypairs[14], &keypairs[15]]
        );
    }

    #[test]
    fn test_bench_tps_key_chunks_new_with_conflict_groups() {
        let num_keypairs = 16;
        let chunk_size = 4;
        let num_conflict_groups = 2;
        let keypairs = std::iter::repeat_with(Keypair::new)
            .take(num_keypairs)
            .collect::<Vec<_>>();

        let chunks =
            KeypairChunks::new_with_conflict_groups(&keypairs, chunk_size, num_conflict_groups);
        assert_eq!(
            chunks.source[0],
            &[&keypairs[0], &keypairs[1], &keypairs[2], &keypairs[3]]
        );
        assert_eq!(
            chunks.dest[0],
            &[&keypairs[4], &keypairs[5], &keypairs[4], &keypairs[5]]
        );
        assert_eq!(
            chunks.source[1],
            &[&keypairs[8], &keypairs[9], &keypairs[10], &keypairs[11]]
        );
        assert_eq!(
            chunks.dest[1],
            &[&keypairs[12], &keypairs[13], &keypairs[12], &keypairs[13]]
        );
    }
}
