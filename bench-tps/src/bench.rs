use {
    crate::{
        bench_tps_client::*,
        cli::{Config, InstructionPaddingConfig},
        perf_utils::{sample_txs, SampleStats},
        send_batch::*,
        spl_convert::FromOtherSolana,
    },
    log::*,
    rand::distributions::{Distribution, Uniform},
    rayon::prelude::*,
    solana_client::{nonce_utils, rpc_request::MAX_MULTIPLE_ACCOUNTS},
    solana_metrics::{self, datapoint_info},
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
        timing::{duration_as_ms, duration_as_s, duration_as_us, timestamp},
        transaction::Transaction,
    },
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

// Add prioritization fee to transfer transactions, when `--use-randomized-compute-unit-price`
// is used, compute-unit-price is randomly generated in range of (0..MAX_COMPUTE_UNIT_PRICE).
// It also sets transaction's compute-unit to TRANSFER_TRANSACTION_COMPUTE_UNIT. Therefore the
// max additional cost is `TRANSFER_TRANSACTION_COMPUTE_UNIT * MAX_COMPUTE_UNIT_PRICE / 1_000_000`
const MAX_COMPUTE_UNIT_PRICE: u64 = 50;
const TRANSFER_TRANSACTION_COMPUTE_UNIT: u32 = 200;
/// calculate maximum possible prioritization fee, if `use-randomized-compute-unit-price` is
/// enabled, round to nearest lamports.
pub fn max_lamports_for_prioritization(use_randomized_compute_unit_price: bool) -> u64 {
    if use_randomized_compute_unit_price {
        const MICRO_LAMPORTS_PER_LAMPORT: u64 = 1_000_000;
        let micro_lamport_fee: u128 = (MAX_COMPUTE_UNIT_PRICE as u128)
            .saturating_mul(TRANSFER_TRANSACTION_COMPUTE_UNIT as u128);
        let fee = micro_lamport_fee
            .saturating_add(MICRO_LAMPORTS_PER_LAMPORT.saturating_sub(1) as u128)
            .saturating_div(MICRO_LAMPORTS_PER_LAMPORT as u128);
        u64::try_from(fee).unwrap_or(u64::MAX)
    } else {
        0u64
    }
}

pub type TimestampedTransaction = (Transaction, Option<u64>);
pub type SharedTransactions = Arc<RwLock<VecDeque<Vec<TimestampedTransaction>>>>;

/// Keypairs split into source and destination
/// used for transfer transactions
struct KeypairChunks<'a> {
    source: Vec<Vec<&'a Keypair>>,
    dest: Vec<VecDeque<&'a Keypair>>,
}

impl<'a> KeypairChunks<'a> {
    /// Split input vector of keypairs into two sets of chunks of given size
    fn new(keypairs: &'a [Keypair], chunk_size: usize) -> Self {
        let mut source_keypair_chunks: Vec<Vec<&Keypair>> = Vec::new();
        let mut dest_keypair_chunks: Vec<VecDeque<&Keypair>> = Vec::new();
        for chunk in keypairs.chunks_exact(2 * chunk_size) {
            source_keypair_chunks.push(chunk[..chunk_size].iter().collect());
            dest_keypair_chunks.push(chunk[chunk_size..].iter().collect());
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
    use_randomized_compute_unit_price: bool,
    instruction_padding_config: Option<InstructionPaddingConfig>,
}

impl<'a, 'b, T> TransactionChunkGenerator<'a, 'b, T>
where
    T: 'static + BenchTpsClient + Send + Sync + ?Sized,
{
    fn new(
        client: Arc<T>,
        gen_keypairs: &'a [Keypair],
        nonce_keypairs: Option<&'b Vec<Keypair>>,
        chunk_size: usize,
        use_randomized_compute_unit_price: bool,
        instruction_padding_config: Option<InstructionPaddingConfig>,
    ) -> Self {
        let account_chunks = KeypairChunks::new(gen_keypairs, chunk_size);
        let nonce_chunks =
            nonce_keypairs.map(|nonce_keypairs| KeypairChunks::new(nonce_keypairs, chunk_size));

        TransactionChunkGenerator {
            client,
            account_chunks,
            nonce_chunks,
            chunk_index: 0,
            reclaim_lamports_back_to_source_account: false,
            use_randomized_compute_unit_price,
            instruction_padding_config,
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
                self.use_randomized_compute_unit_price,
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
            duration_as_ms(&duration),
            blockhash,
        );
        datapoint_info!(
            "bench-tps-generate_txs",
            ("duration", duration_as_us(&duration), i64)
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

        // Switch directions after transfering for each "chunk"
        if self.chunk_index == 0 {
            self.reclaim_lamports_back_to_source_account =
                !self.reclaim_lamports_back_to_source_account;
        }
    }
}

fn wait_for_target_slots_per_epoch<T>(target_slots_per_epoch: u64, client: &Arc<T>)
where
    T: 'static + BenchTpsClient + Send + Sync + ?Sized,
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
    exit_signal: &Arc<AtomicBool>,
    sample_period: u64,
    maxes: &Arc<RwLock<Vec<(String, SampleStats)>>>,
) -> JoinHandle<()>
where
    T: 'static + BenchTpsClient + Send + Sync + ?Sized,
{
    info!("Sampling TPS every {} second...", sample_period);
    let exit_signal = exit_signal.clone();
    let maxes = maxes.clone();
    let client = client.clone();
    Builder::new()
        .name("solana-client-sample".to_string())
        .spawn(move || {
            sample_txs(&exit_signal, &maxes, sample_period, &client);
        })
        .unwrap()
}

fn generate_chunked_transfers<T: 'static + BenchTpsClient + Send + Sync + ?Sized>(
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
    exit_signal: &Arc<AtomicBool>,
    shared_tx_active_thread_count: &Arc<AtomicIsize>,
) -> Vec<JoinHandle<()>>
where
    T: 'static + BenchTpsClient + Send + Sync + ?Sized,
{
    (0..threads)
        .map(|_| {
            let exit_signal = exit_signal.clone();
            let shared_txs = shared_txs.clone();
            let shared_tx_active_thread_count = shared_tx_active_thread_count.clone();
            let total_tx_sent_count = total_tx_sent_count.clone();
            let client = client.clone();
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
    T: 'static + BenchTpsClient + Send + Sync + ?Sized,
{
    let Config {
        id,
        threads,
        thread_batch_sleep_ms,
        duration,
        tx_count,
        sustained,
        target_slots_per_epoch,
        use_randomized_compute_unit_price,
        use_durable_nonce,
        instruction_padding_config,
        ..
    } = config;

    assert!(gen_keypairs.len() >= 2 * tx_count);
    let chunk_generator = TransactionChunkGenerator::new(
        client.clone(),
        &gen_keypairs,
        nonce_keypairs.as_ref(),
        tx_count,
        use_randomized_compute_unit_price,
        instruction_padding_config,
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
    let sample_thread = create_sampler_thread(&client, &exit_signal, sample_period, &maxes);

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

    let s_threads = create_sender_threads(
        &client,
        &shared_txs,
        thread_batch_sleep_ms,
        &total_tx_sent_count,
        threads,
        &exit_signal,
        &shared_tx_active_thread_count,
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
    for t in s_threads {
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
    use_randomized_compute_unit_price: bool,
) -> Vec<TimestampedTransaction> {
    let pairs: Vec<_> = if !reclaim {
        source.iter().zip(dest.iter()).collect()
    } else {
        dest.iter().zip(source.iter()).collect()
    };

    if use_randomized_compute_unit_price {
        let mut rng = rand::thread_rng();
        let range = Uniform::from(0..MAX_COMPUTE_UNIT_PRICE);
        let compute_unit_prices: Vec<_> =
            (0..pairs.len()).map(|_| range.sample(&mut rng)).collect();
        let pairs_with_compute_unit_prices: Vec<_> =
            pairs.iter().zip(compute_unit_prices.iter()).collect();

        pairs_with_compute_unit_prices
            .par_iter()
            .map(|((from, to), compute_unit_price)| {
                (
                    transfer_with_compute_unit_price_and_padding(
                        from,
                        &to.pubkey(),
                        1,
                        *blockhash,
                        instruction_padding_config,
                        Some(**compute_unit_price),
                    ),
                    Some(timestamp()),
                )
            })
            .collect()
    } else {
        pairs
            .par_iter()
            .map(|(from, to)| {
                (
                    transfer_with_compute_unit_price_and_padding(
                        from,
                        &to.pubkey(),
                        1,
                        *blockhash,
                        instruction_padding_config,
                        None,
                    ),
                    Some(timestamp()),
                )
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
) -> Transaction {
    let from_pubkey = from_keypair.pubkey();
    let transfer_instruction = system_instruction::transfer(&from_pubkey, to, lamports);
    let instruction = if let Some(instruction_padding_config) = instruction_padding_config {
        FromOtherSolana::from(
            wrap_instruction(
                FromOtherSolana::from(instruction_padding_config.program_id),
                FromOtherSolana::from(transfer_instruction),
                vec![],
                instruction_padding_config.data_size,
            )
            .expect("Could not create padded instruction"),
        )
    } else {
        transfer_instruction
    };
    let mut instructions = vec![instruction];
    if let Some(compute_unit_price) = compute_unit_price {
        instructions.extend_from_slice(&[
            ComputeBudgetInstruction::set_compute_unit_limit(TRANSFER_TRANSACTION_COMPUTE_UNIT),
            ComputeBudgetInstruction::set_compute_unit_price(compute_unit_price),
        ])
    }
    let message = Message::new(&instructions, Some(&from_pubkey));
    Transaction::new(&[from_keypair], message, recent_blockhash)
}

fn get_nonce_accounts<T: 'static + BenchTpsClient + Send + Sync + ?Sized>(
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

fn get_nonce_blockhashes<T: 'static + BenchTpsClient + Send + Sync + ?Sized>(
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
    instruction_padding_config: &Option<InstructionPaddingConfig>,
) -> Transaction {
    let from_pubkey = from_keypair.pubkey();
    let transfer_instruction = system_instruction::transfer(&from_pubkey, to, lamports);
    let instruction = if let Some(instruction_padding_config) = instruction_padding_config {
        FromOtherSolana::from(
            wrap_instruction(
                FromOtherSolana::from(instruction_padding_config.program_id),
                FromOtherSolana::from(transfer_instruction),
                vec![],
                instruction_padding_config.data_size,
            )
            .expect("Could not create padded instruction"),
        )
    } else {
        transfer_instruction
    };
    let message = Message::new_with_nonce(
        vec![instruction],
        Some(&from_pubkey),
        nonce_account,
        &nonce_authority.pubkey(),
    );
    Transaction::new(&[from_keypair, nonce_authority], message, nonce_hash)
}

fn generate_nonced_system_txs<T: 'static + BenchTpsClient + Send + Sync + ?Sized>(
    client: Arc<T>,
    source: &[&Keypair],
    dest: &VecDeque<&Keypair>,
    source_nonce: &[&Keypair],
    dest_nonce: &VecDeque<&Keypair>,
    reclaim: bool,
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
            transactions.push((
                nonced_transfer_with_padding(
                    source[i],
                    &dest[i].pubkey(),
                    1,
                    &source_nonce[i].pubkey(),
                    source[i],
                    blockhashes[i],
                    instruction_padding_config,
                ),
                None,
            ));
        }
    } else {
        let pubkeys: Vec<Pubkey> = dest_nonce.iter().map(|keypair| keypair.pubkey()).collect();
        let blockhashes: Vec<Hash> = get_nonce_blockhashes(&client, &pubkeys);

        for i in 0..length {
            transactions.push((
                nonced_transfer_with_padding(
                    dest[i],
                    &source[i].pubkey(),
                    1,
                    &dest_nonce[i].pubkey(),
                    dest[i],
                    blockhashes[i],
                    instruction_padding_config,
                ),
                None,
            ));
        }
    }
    transactions
}

fn generate_txs<T: 'static + BenchTpsClient + Send + Sync + ?Sized>(
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

fn get_new_latest_blockhash<T: BenchTpsClient + ?Sized>(
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

fn poll_blockhash<T: BenchTpsClient + ?Sized>(
    exit_signal: &Arc<AtomicBool>,
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

fn do_tx_transfers<T: BenchTpsClient + ?Sized>(
    exit_signal: &Arc<AtomicBool>,
    shared_txs: &SharedTransactions,
    shared_tx_thread_count: &Arc<AtomicIsize>,
    total_tx_sent_count: &Arc<AtomicUsize>,
    thread_batch_sleep_ms: usize,
    client: &Arc<T>,
) {
    let mut last_sent_time = timestamp();
    loop {
        if thread_batch_sleep_ms > 0 {
            sleep(Duration::from_millis(thread_batch_sleep_ms as u64));
        }
        let txs = {
            let mut shared_txs_wl = shared_txs.write().expect("write lock in do_tx_transfers");
            shared_txs_wl.pop_front()
        };
        if let Some(txs0) = txs {
            shared_tx_thread_count.fetch_add(1, Ordering::Relaxed);
            info!("Transferring 1 unit {} times...", txs0.len());
            let tx_len = txs0.len();
            let transfer_start = Instant::now();
            let mut old_transactions = false;
            let mut transactions = Vec::<_>::new();
            let mut min_timestamp = u64::MAX;
            for tx in txs0 {
                let now = timestamp();
                // Transactions without durable nonce that are too old will be rejected by the cluster Don't bother
                // sending them.
                if let Some(tx_timestamp) = tx.1 {
                    if tx_timestamp < min_timestamp {
                        min_timestamp = tx_timestamp;
                    }
                    if now > tx_timestamp && now - tx_timestamp > 1000 * MAX_TX_QUEUE_AGE {
                        old_transactions = true;
                        continue;
                    }
                }
                transactions.push(tx.0);
            }

            if min_timestamp != u64::MAX {
                datapoint_info!(
                    "bench-tps-do_tx_transfers",
                    ("oldest-blockhash-age", timestamp() - min_timestamp, i64),
                );
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
            total_tx_sent_count.fetch_add(tx_len, Ordering::Relaxed);
            info!(
                "Tx send done. {} ms {} tps",
                duration_as_ms(&transfer_start.elapsed()),
                tx_len as f32 / duration_as_s(&transfer_start.elapsed()),
            );
            datapoint_info!(
                "bench-tps-do_tx_transfers",
                ("duration", duration_as_us(&transfer_start.elapsed()), i64),
                ("count", tx_len, i64)
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
        max_tx_count as f32 / duration_as_s(tx_send_elapsed)
    );
}

pub fn generate_and_fund_keypairs<T: 'static + BenchTpsClient + Send + Sync + ?Sized>(
    client: Arc<T>,
    funding_key: &Keypair,
    keypair_count: usize,
    lamports_per_account: u64,
) -> Result<Vec<Keypair>> {
    let rent = client.get_minimum_balance_for_rent_exemption(0)?;
    let lamports_per_account = lamports_per_account + rent;

    info!("Creating {} keypairs...", keypair_count);
    let (mut keypairs, extra) = generate_keypairs(funding_key, keypair_count as u64);
    fund_keypairs(client, funding_key, &keypairs, extra, lamports_per_account)?;

    // 'generate_keypairs' generates extra keys to be able to have size-aligned funding batches for fund_keys.
    keypairs.truncate(keypair_count);

    Ok(keypairs)
}

pub fn fund_keypairs<T: 'static + BenchTpsClient + Send + Sync + ?Sized>(
    client: Arc<T>,
    funding_key: &Keypair,
    keypairs: &[Keypair],
    extra: u64,
    lamports_per_account: u64,
) -> Result<()> {
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
                return Err(BenchTpsError::AirdropFailure);
            }
        }

        fund_keys(
            client,
            funding_key,
            keypairs,
            total,
            max_fee,
            lamports_per_account,
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        solana_runtime::{bank::Bank, bank_client::BankClient},
        solana_sdk::{
            commitment_config::CommitmentConfig, fee_calculator::FeeRateGovernor,
            genesis_config::create_genesis_config, native_token::sol_to_lamports, nonce::State,
        },
    };

    #[test]
    fn test_bench_tps_bank_client() {
        let (genesis_config, id) = create_genesis_config(sol_to_lamports(10_000.0));
        let bank = Bank::new_for_tests(&genesis_config);
        let client = Arc::new(BankClient::new(bank));

        let config = Config {
            id,
            tx_count: 10,
            duration: Duration::from_secs(5),
            ..Config::default()
        };

        let keypair_count = config.tx_count * config.keypair_multiplier;
        let keypairs =
            generate_and_fund_keypairs(client.clone(), &config.id, keypair_count, 20).unwrap();

        do_bench_tps(client, config, keypairs, None);
    }

    #[test]
    fn test_bench_tps_fund_keys() {
        let (genesis_config, id) = create_genesis_config(sol_to_lamports(10_000.0));
        let bank = Bank::new_for_tests(&genesis_config);
        let client = Arc::new(BankClient::new(bank));
        let keypair_count = 20;
        let lamports = 20;
        let rent = client.get_minimum_balance_for_rent_exemption(0).unwrap();

        let keypairs =
            generate_and_fund_keypairs(client.clone(), &id, keypair_count, lamports).unwrap();

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
        let bank = Bank::new_for_tests(&genesis_config);
        let client = Arc::new(BankClient::new(bank));
        let keypair_count = 20;
        let lamports = 20;
        let rent = client.get_minimum_balance_for_rent_exemption(0).unwrap();

        let keypairs =
            generate_and_fund_keypairs(client.clone(), &id, keypair_count, lamports).unwrap();

        for kp in &keypairs {
            assert_eq!(client.get_balance(&kp.pubkey()).unwrap(), lamports + rent);
        }
    }

    #[test]
    fn test_bench_tps_create_durable_nonce() {
        let (genesis_config, id) = create_genesis_config(sol_to_lamports(10_000.0));
        let bank = Bank::new_for_tests(&genesis_config);
        let client = Arc::new(BankClient::new(bank));
        let keypair_count = 10;
        let lamports = 10_000_000;

        let authority_keypairs =
            generate_and_fund_keypairs(client.clone(), &id, keypair_count, lamports).unwrap();

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
}
