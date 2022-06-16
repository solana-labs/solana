use rayon::iter::split;

use {
    crate::{
        bench_tps_client::*,
        cli::Config,
        perf_utils::{sample_txs, SampleStats},
    },
    log::*,
    rayon::prelude::*,
    solana_client::nonce_utils,
    solana_core::gen_keys::GenKeys,
    solana_measure::measure::Measure,
    solana_metrics::{self, datapoint_info},
    solana_sdk::{
        clock::{DEFAULT_MS_PER_SLOT, DEFAULT_S_PER_SLOT, MAX_PROCESSING_AGE},
        commitment_config::CommitmentConfig,
        hash::Hash,
        instruction::{AccountMeta, Instruction},
        message::Message,
        native_token::Sol,
        nonce::State,
        pubkey::Pubkey,
        signature::{Keypair, Signer},
        system_instruction, system_transaction,
        timing::{duration_as_ms, duration_as_s, duration_as_us, timestamp},
        transaction::Transaction,
    },
    std::{
        collections::{HashSet, VecDeque},
        process::exit,
        sync::{
            atomic::{AtomicBool, AtomicIsize, AtomicUsize, Ordering},
            Arc, Mutex, RwLock,
        },
        thread,
        thread::{sleep, Builder, JoinHandle},
        time::{Duration, Instant},
    },
};

// The point at which transactions become "too old", in seconds.
const MAX_TX_QUEUE_AGE: u64 = (MAX_PROCESSING_AGE as f64 * DEFAULT_S_PER_SLOT) as u64;

pub const MAX_SPENDS_PER_TX: u64 = 4;

pub type SharedTransactions = Arc<RwLock<VecDeque<Vec<(Transaction, u64)>>>>;

fn get_latest_blockhash<T: BenchTpsClient>(client: &T) -> Hash {
    loop {
        match client.get_latest_blockhash_with_commitment(CommitmentConfig::processed()) {
            Ok((blockhash, _)) => return blockhash,
            Err(err) => {
                info!("Couldn't get last blockhash: {:?}", err);
                sleep(Duration::from_secs(1));
            }
        };
    }
}

/// Split input vector of keypairs into two sets of chunks of given size
fn split_into_source_destination(
    keypairs: &Vec<Keypair>,
    chunk_size: usize,
) -> (Vec<Vec<&Keypair>>, Vec<VecDeque<&Keypair>>) {
    let mut source_keypair_chunks: Vec<Vec<&Keypair>> = Vec::new();
    let mut dest_keypair_chunks: Vec<VecDeque<&Keypair>> = Vec::new();
    for chunk in keypairs.chunks_exact(2 * chunk_size) {
        source_keypair_chunks.push(chunk[..chunk_size].iter().collect());
        dest_keypair_chunks.push(chunk[chunk_size..].iter().collect());
    }
    (source_keypair_chunks, dest_keypair_chunks)
}

fn wait_for_target_slots_per_epoch<T>(target_slots_per_epoch: u64, client: &Arc<T>)
where
    T: 'static + BenchTpsClient + Send + Sync,
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
    T: 'static + BenchTpsClient + Send + Sync,
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

fn generate_chunked_transfers<T: 'static + BenchTpsClient + Send + Sync>(
    client: Arc<T>,
    recent_blockhash: Arc<RwLock<Hash>>,
    shared_txs: &SharedTransactions,
    shared_tx_active_thread_count: Arc<AtomicIsize>,
    source_keypair_chunks: Vec<Vec<&Keypair>>,
    dest_keypair_chunks: &mut [VecDeque<&Keypair>],
    source_nonce_keypair_chunks: Vec<Vec<&Keypair>>,
    dest_nonce_keypair_chunks: &mut [VecDeque<&Keypair>],
    threads: usize,
    duration: Duration,
    sustained: bool,
) {
    // generate and send transactions for the specified duration
    let start = Instant::now();
    let keypair_chunks = source_keypair_chunks.len();
    let mut reclaim_lamports_back_to_source_account = false;
    let mut chunk_index = 0;
    let mut last_generate_txs_time = Instant::now();

    while start.elapsed() < duration {
        let source_nonce_chunk = &source_nonce_keypair_chunks[chunk_index];
        let dest_nonce_chunk: &VecDeque<&Keypair> = &dest_nonce_keypair_chunks[chunk_index];
        generate_txs(
            client.clone(), // TODO(klykov) any need in clonning?
            shared_txs,
            &recent_blockhash,
            &source_keypair_chunks[chunk_index],
            &dest_keypair_chunks[chunk_index],
            source_nonce_chunk,
            dest_nonce_chunk,
            true, // TODO(klykov) for now always
            threads,
            reclaim_lamports_back_to_source_account,
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

        // Rotate destination keypairs so that the next round of transactions will have different
        // transaction signatures even when blockhash is reused.
        dest_keypair_chunks[chunk_index].rotate_left(1);
        dest_nonce_keypair_chunks[chunk_index].rotate_left(1); // TODO(klykov): refactor later

        // Move on to next chunk
        chunk_index = (chunk_index + 1) % keypair_chunks;

        // Switch directions after transfering for each "chunk"
        if chunk_index == 0 {
            reclaim_lamports_back_to_source_account = !reclaim_lamports_back_to_source_account;
        }
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
    T: 'static + BenchTpsClient + Send + Sync,
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

// wait until all accounts creation is finalized
fn wait_for_commitment<T, Predicate>(client: Arc<T>, keypairs: &Vec<Keypair>, predicate: Predicate)
where
    T: 'static + BenchTpsClient + Send + Sync,
    Predicate: Fn(u64) -> bool,
{
    use std::collections::HashSet;

    //let client = client.rpc_client();
    let mut uncommitted = HashSet::<usize>::from_iter((0..keypairs.len()).into_iter());
    let mut tobe_removed = Vec::<usize>::new();
    while !uncommitted.is_empty() {
        for index in &uncommitted {
            let res = client.get_balance_with_commitment(
                &keypairs[*index].pubkey(),
                CommitmentConfig::finalized(),
            );
            //let res = client.get_account_with_commitment(
            //    &keypairs[*index].pubkey(),
            //    CommitmentConfig::finalized(),
            //);
            if let Ok(res) = res {
                if predicate(res) {
                    tobe_removed.push(*index);
                }
            }
        }
        for index in &tobe_removed {
            uncommitted.remove(index);
        }
        tobe_removed.clear();
        info!("@ num uncommitted: {}", uncommitted.len());
        thread::sleep(std::time::Duration::from_secs(1));
    }
}

fn generate_durable_nonce_accounts<T: 'static + BenchTpsClient + Send + Sync>(
    client: Arc<T>,
    authority_keypairs: &Vec<Keypair>,
) -> Result<Vec<Keypair>> {
    let nonce_rent = client
        .get_minimum_balance_for_rent_exemption(State::size())
        .unwrap();

    //let client = client.rpc_client(); // TODO remove later, for now it gives better errors
    let seed_keypair = &authority_keypairs[0];
    let count = authority_keypairs.len();
    let (mut nonce_keypairs, _extra) = generate_keypairs(&seed_keypair, count as u64);
    nonce_keypairs.truncate(count);
    for (authority, nonce) in authority_keypairs.iter().zip(nonce_keypairs.iter()) {
        // make
        let instr = system_instruction::create_nonce_account(
            &authority.pubkey(),
            &nonce.pubkey(),
            &authority.pubkey(),
            nonce_rent,
        );

        let mut tx = Transaction::new_with_payer(&instr, Some(&authority.pubkey()));
        // sign
        let blockhash = get_latest_blockhash(client.as_ref());
        if tx.try_sign(&[&nonce, authority], blockhash).is_err() {
            return Err(BenchTpsError::Custom("Signing has failed".to_string()));
        }
        // send
        let result = client.send_transaction(tx)?;
    }
    Ok(nonce_keypairs)
}

pub fn do_bench_tps<T>(client: Arc<T>, config: Config, gen_keypairs: Vec<Keypair>) -> u64
where
    T: 'static + BenchTpsClient + Send + Sync,
{
    let Config {
        id,
        threads,
        thread_batch_sleep_ms,
        duration,
        tx_count,
        sustained,
        target_slots_per_epoch,
        ..
    } = config;

    assert!(gen_keypairs.len() >= 2 * tx_count);
    assert!(config.use_durable_nonce); // TODO(klykov) for now, later move to structure
    let (source_keypair_chunks, mut dest_keypair_chunks) =
        split_into_source_destination(&gen_keypairs, tx_count);

    let nonce_keypairs =
        generate_durable_nonce_accounts(client.clone(), &gen_keypairs)
            .unwrap_or_else(|error| panic!("Failed to generate durable accounts: {:?}", error));
    let (source_nonce_keypair_chunks, mut dest_nonce_keypair_chunks) =
        split_into_source_destination(&nonce_keypairs, tx_count);

    info!("BEFORE");
    wait_for_commitment(client.clone(), &nonce_keypairs, |lamports: u64| {
        lamports > 0
    });
    info!("AFTER");

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

    // if we use durable nonce, we don't need blockhash
    let blockhash_thread = if !config.use_durable_nonce {
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
        client.clone(), // TODO(klykov): this is needed only for nonce, move later to a struct
        blockhash,
        &shared_txs,
        shared_tx_active_thread_count,
        source_keypair_chunks,
        &mut dest_keypair_chunks,
        source_nonce_keypair_chunks,
        &mut dest_nonce_keypair_chunks,
        threads,
        duration,
        sustained,
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
) -> Vec<(Transaction, u64)> {
    let pairs: Vec<_> = if !reclaim {
        source.iter().zip(dest.iter()).collect()
    } else {
        dest.iter().zip(source.iter()).collect()
    };

    pairs
        .par_iter()
        .map(|(from, to)| {
            (
                system_transaction::transfer(from, &to.pubkey(), 1, *blockhash),
                timestamp(),
            )
        })
        .collect()
}

// TODO(klykov): use Result later
fn get_nonce_blockhash<T: 'static + BenchTpsClient + Send + Sync>(
    client: Arc<T>,
    nonce_account_pubkey: Pubkey,
) -> Hash {
    let nonce_account = client
        .get_account(&nonce_account_pubkey)
        .unwrap_or_else(|error| panic!("{:?}", error));
    let nonce_data = nonce_utils::data_from_account(&nonce_account)
        .unwrap_or_else(|error| panic!("{:?}", error));
    nonce_data.blockhash()
}

fn generate_nonced_system_txs<T: 'static + BenchTpsClient + Send + Sync>(
    client: Arc<T>,
    source: &[&Keypair],
    dest: &VecDeque<&Keypair>,
    source_nonce: &[&Keypair],
    dest_nonce: &VecDeque<&Keypair>,
    reclaim: bool,
) -> Vec<(Transaction, u64)> {
    let length = source.len();
    let mut transactions: Vec<(Transaction, u64)> = Vec::with_capacity(length);
    for i in 0..length {
        let (from, to, nonce, nonce_blockhash) = if !reclaim {
            (
                source[i],
                dest[i],
                source_nonce[i],
                get_nonce_blockhash(client.clone(), source_nonce[i].pubkey()),
            )
        } else {
            (
                dest[i],
                source[i],
                dest_nonce[i],
                get_nonce_blockhash(client.clone(), dest_nonce[i].pubkey()),
            )
        };

        transactions.push((
            system_transaction::nonced_transfer(
                from,
                &to.pubkey(),
                1,
                &nonce.pubkey(),
                from,
                nonce_blockhash,
            ),
            timestamp(),
        ));
    }
    // current timestamp to avoid filtering out some transactions if they are too old
    let t = timestamp();
    for mut tx in &mut transactions {
        tx.1 = t;
    }
    transactions
}

fn generate_txs<T: 'static + BenchTpsClient + Send + Sync>(
    client: Arc<T>,
    shared_txs: &SharedTransactions,
    blockhash: &Arc<RwLock<Hash>>,
    source: &[&Keypair],
    dest: &VecDeque<&Keypair>,
    source_nonce: &[&Keypair],
    dest_nonce: &VecDeque<&Keypair>,
    durable_nonce: bool,
    threads: usize,
    reclaim: bool,
) {
    let blockhash = *blockhash.read().unwrap();
    let tx_count = source.len();
    info!(
        "Signing transactions... {} (reclaim={}, blockhash={})",
        tx_count, reclaim, &blockhash
    );
    let signing_start = Instant::now();

    let transactions = if durable_nonce {
        generate_nonced_system_txs(client, source, dest, source_nonce, dest_nonce, reclaim)
    } else {
        generate_system_txs(source, dest, reclaim, &blockhash)
    };

    let duration = signing_start.elapsed();
    let ns = duration.as_secs() * 1_000_000_000 + u64::from(duration.subsec_nanos());
    let bsps = (tx_count) as f64 / ns as f64;
    let nsps = ns as f64 / (tx_count) as f64;
    info!(
        "Done. {:.2} thousand signatures per second, {:.2} us per signature, {} ms total time, {}",
        bsps * 1_000_000_f64,
        nsps / 1_000_f64,
        duration_as_ms(&duration),
        blockhash,
    );
    datapoint_info!(
        "bench-tps-generate_txs",
        ("duration", duration_as_us(&duration), i64)
    );

    let sz = transactions.len() / threads;
    let chunks: Vec<_> = transactions.chunks(sz).collect();
    {
        let mut shared_txs_wl = shared_txs.write().unwrap();
        for chunk in chunks {
            shared_txs_wl.push_back(chunk.to_vec());
        }
    }
}

fn get_new_latest_blockhash<T: BenchTpsClient>(client: &Arc<T>, blockhash: &Hash) -> Option<Hash> {
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

fn poll_blockhash<T: BenchTpsClient>(
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

fn do_tx_transfers<T: BenchTpsClient>(
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
                // Transactions that are too old will be rejected by the cluster Don't bother
                // sending them.
                if tx.1 < min_timestamp {
                    min_timestamp = tx.1;
                }
                if now > tx.1 && now - tx.1 > 1000 * MAX_TX_QUEUE_AGE {
                    old_transactions = true;
                    continue;
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

fn verify_funding_transfer<T: BenchTpsClient>(
    client: &Arc<T>,
    tx: &Transaction,
    amount: u64,
) -> bool {
    for a in &tx.message().account_keys[1..] {
        match client.get_balance_with_commitment(a, CommitmentConfig::processed()) {
            Ok(balance) => return balance >= amount,
            Err(err) => error!("failed to get balance {:?}", err),
        }
    }
    false
}

trait FundingTransactions<'a> {
    fn fund<T: 'static + BenchTpsClient + Send + Sync>(
        &mut self,
        client: &Arc<T>,
        to_fund: &[(&'a Keypair, Vec<(Pubkey, u64)>)],
        to_lamports: u64,
    );
    fn make(&mut self, to_fund: &[(&'a Keypair, Vec<(Pubkey, u64)>)]);
    fn sign(&mut self, blockhash: Hash);
    fn send<T: BenchTpsClient>(&self, client: &Arc<T>);
    fn verify<T: 'static + BenchTpsClient + Send + Sync>(
        &mut self,
        client: &Arc<T>,
        to_lamports: u64,
    );
}

impl<'a> FundingTransactions<'a> for Vec<(&'a Keypair, Transaction)> {
    fn fund<T: 'static + BenchTpsClient + Send + Sync>(
        &mut self,
        client: &Arc<T>,
        to_fund: &[(&'a Keypair, Vec<(Pubkey, u64)>)],
        to_lamports: u64,
    ) {
        self.make(to_fund);

        let mut tries = 0;
        while !self.is_empty() {
            info!(
                "{} {} each to {} accounts in {} txs",
                if tries == 0 {
                    "transferring"
                } else {
                    " retrying"
                },
                to_lamports,
                self.len() * MAX_SPENDS_PER_TX as usize,
                self.len(),
            );

            let blockhash = get_latest_blockhash(client.as_ref());

            // re-sign retained to_fund_txes with updated blockhash
            self.sign(blockhash);
            self.send(client);

            // Sleep a few slots to allow transactions to process
            sleep(Duration::from_secs(1));

            self.verify(client, to_lamports);

            // retry anything that seems to have dropped through cracks
            //  again since these txs are all or nothing, they're fine to
            //  retry
            tries += 1;
        }
        info!("transferred");
    }

    fn make(&mut self, to_fund: &[(&'a Keypair, Vec<(Pubkey, u64)>)]) {
        let mut make_txs = Measure::start("make_txs");
        let to_fund_txs: Vec<(&Keypair, Transaction)> = to_fund
            .par_iter()
            .map(|(k, t)| {
                let instructions = system_instruction::transfer_many(&k.pubkey(), t);
                let message = Message::new(&instructions, Some(&k.pubkey()));
                (*k, Transaction::new_unsigned(message))
            })
            .collect();
        make_txs.stop();
        debug!(
            "make {} unsigned txs: {}us",
            to_fund_txs.len(),
            make_txs.as_us()
        );
        self.extend(to_fund_txs);
    }

    fn sign(&mut self, blockhash: Hash) {
        let mut sign_txs = Measure::start("sign_txs");
        self.par_iter_mut().for_each(|(k, tx)| {
            tx.sign(&[*k], blockhash);
        });
        sign_txs.stop();
        debug!("sign {} txs: {}us", self.len(), sign_txs.as_us());
    }

    fn send<T: BenchTpsClient>(&self, client: &Arc<T>) {
        let mut send_txs = Measure::start("send_and_clone_txs");
        let batch: Vec<_> = self.iter().map(|(_keypair, tx)| tx.clone()).collect();
        client.send_batch(batch).expect("transfer");
        send_txs.stop();
        debug!("send {} {}", self.len(), send_txs);
    }

    fn verify<T: 'static + BenchTpsClient + Send + Sync>(
        &mut self,
        client: &Arc<T>,
        to_lamports: u64,
    ) {
        let starting_txs = self.len();
        let verified_txs = Arc::new(AtomicUsize::new(0));
        let too_many_failures = Arc::new(AtomicBool::new(false));
        let loops = if starting_txs < 1000 { 3 } else { 1 };
        // Only loop multiple times for small (quick) transaction batches
        let time = Arc::new(Mutex::new(Instant::now()));
        for _ in 0..loops {
            let time = time.clone();
            let failed_verify = Arc::new(AtomicUsize::new(0));
            let client = client.clone();
            let verified_txs = &verified_txs;
            let failed_verify = &failed_verify;
            let too_many_failures = &too_many_failures;
            let verified_set: HashSet<Pubkey> = self
                .par_iter()
                .filter_map(move |(k, tx)| {
                    if too_many_failures.load(Ordering::Relaxed) {
                        return None;
                    }

                    let verified = if verify_funding_transfer(&client, tx, to_lamports) {
                        verified_txs.fetch_add(1, Ordering::Relaxed);
                        Some(k.pubkey())
                    } else {
                        failed_verify.fetch_add(1, Ordering::Relaxed);
                        None
                    };

                    let verified_txs = verified_txs.load(Ordering::Relaxed);
                    let failed_verify = failed_verify.load(Ordering::Relaxed);
                    let remaining_count = starting_txs.saturating_sub(verified_txs + failed_verify);
                    if failed_verify > 100 && failed_verify > verified_txs {
                        too_many_failures.store(true, Ordering::Relaxed);
                        warn!(
                            "Too many failed transfers... {} remaining, {} verified, {} failures",
                            remaining_count, verified_txs, failed_verify
                        );
                    }
                    if remaining_count > 0 {
                        let mut time_l = time.lock().unwrap();
                        if time_l.elapsed().as_secs() > 2 {
                            info!(
                                "Verifying transfers... {} remaining, {} verified, {} failures",
                                remaining_count, verified_txs, failed_verify
                            );
                            *time_l = Instant::now();
                        }
                    }

                    verified
                })
                .collect();

            self.retain(|(k, _)| !verified_set.contains(&k.pubkey()));
            if self.is_empty() {
                break;
            }
            info!("Looping verifications");

            let verified_txs = verified_txs.load(Ordering::Relaxed);
            let failed_verify = failed_verify.load(Ordering::Relaxed);
            let remaining_count = starting_txs.saturating_sub(verified_txs + failed_verify);
            info!(
                "Verifying transfers... {} remaining, {} verified, {} failures",
                remaining_count, verified_txs, failed_verify
            );
            sleep(Duration::from_millis(100));
        }
    }
}

/// fund the dests keys by spending all of the source keys into MAX_SPENDS_PER_TX
/// on every iteration.  This allows us to replay the transfers because the source is either empty,
/// or full
pub fn fund_keys<T: 'static + BenchTpsClient + Send + Sync>(
    client: Arc<T>,
    source: &Keypair,
    dests: &[Keypair],
    total: u64,
    max_fee: u64,
    lamports_per_account: u64,
) {
    let mut funded: Vec<&Keypair> = vec![source];
    let mut funded_funds = total;
    let mut not_funded: Vec<&Keypair> = dests.iter().collect();
    while !not_funded.is_empty() {
        // Build to fund list and prepare funding sources for next iteration
        let mut new_funded: Vec<&Keypair> = vec![];
        let mut to_fund: Vec<(&Keypair, Vec<(Pubkey, u64)>)> = vec![];
        let to_lamports = (funded_funds - lamports_per_account - max_fee) / MAX_SPENDS_PER_TX;
        for f in funded {
            let start = not_funded.len() - MAX_SPENDS_PER_TX as usize;
            let dests: Vec<_> = not_funded.drain(start..).collect();
            let spends: Vec<_> = dests.iter().map(|k| (k.pubkey(), to_lamports)).collect();
            to_fund.push((f, spends));
            new_funded.extend(dests.into_iter());
        }

        // try to transfer a "few" at a time with recent blockhash
        //  assume 4MB network buffers, and 512 byte packets
        const FUND_CHUNK_LEN: usize = 4 * 1024 * 1024 / 512;

        to_fund.chunks(FUND_CHUNK_LEN).for_each(|chunk| {
            Vec::<(&Keypair, Transaction)>::with_capacity(chunk.len()).fund(
                &client,
                chunk,
                to_lamports,
            );
        });

        info!("funded: {} left: {}", new_funded.len(), not_funded.len());
        funded = new_funded;
        funded_funds = to_lamports;
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

pub fn generate_keypairs(seed_keypair: &Keypair, count: u64) -> (Vec<Keypair>, u64) {
    let mut seed = [0u8; 32];
    seed.copy_from_slice(&seed_keypair.to_bytes()[..32]);
    let mut rnd = GenKeys::new(seed);

    let mut total_keys = 0;
    let mut extra = 0; // This variable tracks the number of keypairs needing extra transaction fees funded
    let mut delta = 1;
    while total_keys < count {
        extra += delta;
        delta *= MAX_SPENDS_PER_TX;
        total_keys += delta;
    }
    (rnd.gen_n_keypairs(total_keys), extra)
}

pub fn generate_and_fund_keypairs<T: 'static + BenchTpsClient + Send + Sync>(
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

pub fn fund_keypairs<T: 'static + BenchTpsClient + Send + Sync>(
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
    use std::slice::SplitInclusive;

    use {
        super::*,
        itertools::izip,
        solana_client::nonce_utils,
        solana_client::thin_client::{create_client, ThinClient},
        solana_core::validator::ValidatorConfig,
        solana_faucet::faucet::run_local_faucet,
        solana_local_cluster::{
            cluster::Cluster,
            local_cluster::{ClusterConfig, LocalCluster},
            validator_configs::make_identical_validator_configs,
        },
        solana_rpc::rpc::JsonRpcConfig,
        solana_runtime::{bank::Bank, bank_client::BankClient},
        solana_sdk::{
            fee_calculator::FeeRateGovernor, genesis_config::create_genesis_config,
            native_token::sol_to_lamports,
        },
        solana_streamer::socket::SocketAddrSpace,
        std::{thread, time},
    };

    #[test]
    fn test_create_durable_nonce_account() {
        solana_logger::setup();

        // 1. Create faucet thread
        let faucet_keypair = Keypair::new();
        let faucet_pubkey = faucet_keypair.pubkey();
        let faucet_addr = run_local_faucet(faucet_keypair, None);
        let mut validator_config = ValidatorConfig::default_for_test();
        validator_config.rpc_config = JsonRpcConfig {
            faucet_addr: Some(faucet_addr),
            ..JsonRpcConfig::default_for_test()
        };

        // 2. Create a local cluster which is aware of faucet
        let num_nodes = 1;
        let native_instruction_processors = vec![];
        let cluster = LocalCluster::new(
            &mut ClusterConfig {
                node_stakes: vec![999_990; num_nodes],
                cluster_lamports: 200_000_000,
                validator_configs: make_identical_validator_configs(
                    &ValidatorConfig {
                        rpc_config: JsonRpcConfig {
                            faucet_addr: Some(faucet_addr),
                            ..JsonRpcConfig::default_for_test()
                        },
                        ..ValidatorConfig::default_for_test()
                    },
                    num_nodes,
                ),
                native_instruction_processors,
                ..ClusterConfig::default()
            },
            SocketAddrSpace::Unspecified,
        );
        assert_eq!(cluster.validators.len(), num_nodes);

        // 3. Transfer funds to faucet account
        cluster.transfer(&cluster.funding_keypair, &faucet_pubkey, 100_000_000);

        // 4. Create client
        let nodes = cluster.get_node_pubkeys();
        let node = cluster.get_contact_info(&nodes[0]).unwrap().clone();
        let nodes_slice = [node];

        let client = Arc::new(create_client(
            cluster.entry_point_info.rpc,
            cluster.entry_point_info.tpu,
        ));

        let funding_keypair = &cluster.funding_keypair;

        let keypair_count = 2;
        let keypairs =
            generate_and_fund_keypairs(client.clone(), funding_keypair, keypair_count, 100_000)
                .unwrap();

        let nonce_rent = client
            .get_minimum_balance_for_rent_exemption(State::size())
            .unwrap();
        let nonce = Keypair::new();
        let authority = &keypairs[0];

        let client = client.rpc_client();

        let from_balance = client
            .get_balance_with_commitment(&authority.pubkey(), CommitmentConfig::confirmed())
            .unwrap()
            .value;

        loop {
            let res = client
                .get_account_with_commitment(&authority.pubkey(), CommitmentConfig::finalized());
            if let Ok(res) = res {
                if let Some(res) = res.value {
                    if res.lamports > 100_000 {
                        break;
                    }
                }
            }
            thread::sleep(time::Duration::from_secs(10));
        }

        let instr = system_instruction::create_nonce_account(
            &authority.pubkey(),
            &nonce.pubkey(),
            &authority.pubkey(), // Make the fee payer the nonce account authority
            nonce_rent,
        );

        let mut tx = Transaction::new_with_payer(&instr, Some(&authority.pubkey()));
        let blockhash = client.get_latest_blockhash().unwrap();
        if tx.try_sign(&[&nonce, authority], blockhash).is_err() {
            panic!("Signing failed");
        }
        let result = client.send_and_confirm_transaction(&tx);
        if result.is_err() {
            panic!("{:?}", result.err().unwrap());
        }

        let nonce_account = client
            .get_account(&nonce.pubkey())
            .unwrap_or_else(|error| panic!("{:?}", error));
        let nonce_data = nonce_utils::data_from_account(&nonce_account)
            .unwrap_or_else(|error| panic!("{:?}", error));
        let nonce_blockhash = nonce_data.blockhash();

        let from = &keypairs[0];
        let to = &keypairs[1];

        let tx = system_transaction::nonced_transfer(
            from,
            &to.pubkey(),
            100,
            &nonce.pubkey(),
            from,
            nonce_blockhash,
        );

        assert!(client.send_transaction(&tx).is_ok());

        loop {
            let res =
                client.get_account_with_commitment(&to.pubkey(), CommitmentConfig::finalized());
            if let Ok(res) = res {
                if let Some(res) = res.value {
                    info!("@{}", res.lamports);
                    if res.lamports > 100_100 {
                        break;
                    }
                }
            }
            thread::sleep(time::Duration::from_secs(10));
        }
    }

    fn withdraw_nonce_account<T: 'static + BenchTpsClient + Send + Sync>(
        client: Arc<T>,
        keypairs: &Vec<Keypair>,
        nonce_keypairs: &Vec<Keypair>
    ) { 
        // Withdraw and close nonce accounts
        for (nonce, authority) in izip!(nonce_keypairs, keypairs) {
            let nonce_balance = client.get_balance(&nonce.pubkey()).unwrap();
            let instr = system_instruction::withdraw_nonce_account(
                &nonce.pubkey(),
                &authority.pubkey(),
                &authority.pubkey(),
                nonce_balance,
            );

            let mut tx = Transaction::new_with_payer(&[instr], Some(&authority.pubkey()));
            let blockhash = get_latest_blockhash(client.as_ref());
            //let blockhash = client.get_latest_blockhash().ok().unwrap();
            tx.try_sign(&[authority], blockhash);

            let res = client.send_transaction(tx);
            if res.is_err() {
                info!("@{:?}", res.err().unwrap());
            }
            thread::sleep(time::Duration::from_secs(1));
        }
        thread::sleep(time::Duration::from_secs(10));
    }

    #[test]
    fn test_durable_nonce() {
        solana_logger::setup();

        // 1. Create faucet thread
        let faucet_keypair = Keypair::new();
        let faucet_pubkey = faucet_keypair.pubkey();
        let faucet_addr = run_local_faucet(faucet_keypair, None);
        let mut validator_config = ValidatorConfig::default_for_test();
        validator_config.rpc_config = JsonRpcConfig {
            faucet_addr: Some(faucet_addr),
            ..JsonRpcConfig::default_for_test()
        };

        // 2. Create a local cluster which is aware of faucet
        let num_nodes = 1;
        let native_instruction_processors = vec![];
        let cluster = LocalCluster::new(
            &mut ClusterConfig {
                node_stakes: vec![999_990; num_nodes],
                cluster_lamports: 200_000_000,
                validator_configs: make_identical_validator_configs(
                    &ValidatorConfig {
                        rpc_config: JsonRpcConfig {
                            faucet_addr: Some(faucet_addr),
                            ..JsonRpcConfig::default_for_test()
                        },
                        ..ValidatorConfig::default_for_test()
                    },
                    num_nodes,
                ),
                native_instruction_processors,
                ..ClusterConfig::default()
            },
            SocketAddrSpace::Unspecified,
        );
        assert_eq!(cluster.validators.len(), num_nodes);

        // 3. Transfer funds to faucet account
        cluster.transfer(&cluster.funding_keypair, &faucet_pubkey, 100_000_000);

        // 4. Create client
        let nodes = cluster.get_node_pubkeys();
        let node = cluster.get_contact_info(&nodes[0]).unwrap().clone();
        let nodes_slice = [node];

        let client = Arc::new(create_client(
            cluster.entry_point_info.rpc,
            cluster.entry_point_info.tpu,
        ));

        let funding_keypair = &cluster.funding_keypair;
        let tx_count = 2;
        let chunk_count = 1;
        let keypair_count = tx_count * 2 * chunk_count;
        let keypairs =
            generate_and_fund_keypairs(client.clone(), funding_keypair, keypair_count, 100_000)
                .unwrap();

        wait_for_commitment(client.clone(), &keypairs, |lamports: u64| lamports > 10_000);

        let nonce_keypairs = generate_durable_nonce_accounts(client.clone(), &keypairs)
            .unwrap_or_else(|error| panic!("Failed to generate durable accounts: {:?}", error));

        wait_for_commitment(client.clone(), &nonce_keypairs, |lamports: u64| {
            lamports > 0
        });

        let (source_keypair_chunks, dest_keypair_chunks) =
            split_into_source_destination(&keypairs, tx_count);

        // depending on the direction (src->dst or dst->src) we need one or another set
        let (source_nonce_keypair_chunks, dest_nonce_keypair_chunk) =
            split_into_source_destination(&nonce_keypairs, tx_count);

        for (source, dest, source_nonce, dest_nonce) in izip!(
            &source_keypair_chunks,
            &dest_keypair_chunks,
            &source_nonce_keypair_chunks,
            &dest_nonce_keypair_chunk
        ) {
            let transactions_with_timestamps = generate_nonced_system_txs(
                client.clone(),
                source,
                dest,
                source_nonce,
                dest_nonce,
                false,
            );
            let mut transactions = Vec::with_capacity(transactions_with_timestamps.len());
            for (tx, _timestamp) in transactions_with_timestamps {
                transactions.push(tx);
            }
            if let Err(error) = client.send_batch(transactions) {
                println!("send_batch_sync in do_tx_transfers failed: {}", error);
            }
        }
        withdraw_nonce_account(client.clone(), &keypairs, &nonce_keypairs);

        /*let client = client.rpc_client();
        
        // Withdraw and close nonce accounts
        for (nonce, authority) in izip!(&nonce_keypairs, &keypairs) {
            let nonce_balance = client.get_balance(&nonce.pubkey()).unwrap();
            let instr = system_instruction::withdraw_nonce_account(
                &nonce.pubkey(),
                &authority.pubkey(),
                &authority.pubkey(),
                nonce_balance,
            );

            let mut tx = Transaction::new_with_payer(&[instr], Some(&authority.pubkey()));
            //let blockhash = get_latest_blockhash(client.as_ref());
            let blockhash = client.get_latest_blockhash().ok().unwrap();
            assert!(tx.try_sign(&[authority], blockhash).is_ok());

            //assert!(client.send_transaction(tx).is_ok());
            let res = client.send_transaction(&tx);
            if res.is_err() {
                info!("@{:?}", res.err().unwrap());
            }
            thread::sleep(time::Duration::from_secs(1));
        }
        */
        thread::sleep(time::Duration::from_secs(10));
        let client = client.rpc_client();
        // wait until these transactions are finalized
        // otherwise create nonce transactions will fail
        let mut total_committed = nonce_keypairs.len() as u64;
        while total_committed > 0 {
            for to in &nonce_keypairs {
                let res =
                    client.get_account_with_commitment(&to.pubkey(), CommitmentConfig::finalized());
                if let Ok(res) = res {
                    if res.value.is_none() {
                        total_committed -= 1;
                    }
                }
            }
            thread::sleep(time::Duration::from_secs(2));
        }
    }

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

        do_bench_tps(client, config, keypairs);
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
}
