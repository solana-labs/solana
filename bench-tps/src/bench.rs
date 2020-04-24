use crate::cli::Config;
use log::*;
use rayon::prelude::*;
use solana_client::perf_utils::{sample_txs, SampleStats};
use solana_core::gen_keys::GenKeys;
use solana_faucet::faucet::request_airdrop_transaction;
#[cfg(feature = "move")]
use solana_librapay::{create_genesis, upload_mint_script, upload_payment_script};
use solana_measure::measure::Measure;
use solana_metrics::{self, datapoint_info};
use solana_sdk::{
    client::Client,
    clock::{DEFAULT_TICKS_PER_SECOND, DEFAULT_TICKS_PER_SLOT, MAX_PROCESSING_AGE},
    commitment_config::CommitmentConfig,
    fee_calculator::FeeCalculator,
    hash::Hash,
    pubkey::Pubkey,
    signature::{Keypair, Signer},
    system_instruction, system_transaction,
    timing::{duration_as_ms, duration_as_s, duration_as_us, timestamp},
    transaction::Transaction,
};
use std::{
    collections::{HashSet, VecDeque},
    net::SocketAddr,
    process::exit,
    sync::{
        atomic::{AtomicBool, AtomicIsize, AtomicUsize, Ordering},
        Arc, Mutex, RwLock,
    },
    thread::{sleep, Builder},
    time::{Duration, Instant},
};

// The point at which transactions become "too old", in seconds.
const MAX_TX_QUEUE_AGE: u64 =
    MAX_PROCESSING_AGE as u64 * DEFAULT_TICKS_PER_SLOT / DEFAULT_TICKS_PER_SECOND;

#[cfg(feature = "move")]
use solana_librapay::librapay_transaction;

pub const MAX_SPENDS_PER_TX: u64 = 4;

#[derive(Debug)]
pub enum BenchTpsError {
    AirdropFailure,
}

pub type Result<T> = std::result::Result<T, BenchTpsError>;

pub type SharedTransactions = Arc<RwLock<VecDeque<Vec<(Transaction, u64)>>>>;

type LibraKeys = (Keypair, Pubkey, Pubkey, Vec<Keypair>);

fn get_recent_blockhash<T: Client>(client: &T) -> (Hash, FeeCalculator) {
    loop {
        match client.get_recent_blockhash_with_commitment(CommitmentConfig::recent()) {
            Ok((blockhash, fee_calculator)) => return (blockhash, fee_calculator),
            Err(err) => {
                info!("Couldn't get recent blockhash: {:?}", err);
                sleep(Duration::from_secs(1));
            }
        };
    }
}

pub fn do_bench_tps<T>(
    client: Arc<T>,
    config: Config,
    gen_keypairs: Vec<Keypair>,
    libra_args: Option<LibraKeys>,
) -> u64
where
    T: 'static + Client + Send + Sync,
{
    let Config {
        id,
        threads,
        thread_batch_sleep_ms,
        duration,
        tx_count,
        sustained,
        ..
    } = config;

    let mut source_keypair_chunks: Vec<Vec<&Keypair>> = Vec::new();
    let mut dest_keypair_chunks: Vec<VecDeque<&Keypair>> = Vec::new();
    assert!(gen_keypairs.len() >= 2 * tx_count);
    for chunk in gen_keypairs.chunks_exact(2 * tx_count) {
        source_keypair_chunks.push(chunk[..tx_count].iter().collect());
        dest_keypair_chunks.push(chunk[tx_count..].iter().collect());
    }

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
    info!("Sampling TPS every {} second...", sample_period);
    let sample_thread = {
        let exit_signal = exit_signal.clone();
        let maxes = maxes.clone();
        let client = client.clone();
        Builder::new()
            .name("solana-client-sample".to_string())
            .spawn(move || {
                sample_txs(&exit_signal, &maxes, sample_period, &client);
            })
            .unwrap()
    };

    let shared_txs: SharedTransactions = Arc::new(RwLock::new(VecDeque::new()));

    let recent_blockhash = Arc::new(RwLock::new(get_recent_blockhash(client.as_ref()).0));
    let shared_tx_active_thread_count = Arc::new(AtomicIsize::new(0));
    let total_tx_sent_count = Arc::new(AtomicUsize::new(0));

    let blockhash_thread = {
        let exit_signal = exit_signal.clone();
        let recent_blockhash = recent_blockhash.clone();
        let client = client.clone();
        let id = id.pubkey();
        Builder::new()
            .name("solana-blockhash-poller".to_string())
            .spawn(move || {
                poll_blockhash(&exit_signal, &recent_blockhash, &client, &id);
            })
            .unwrap()
    };

    let s_threads: Vec<_> = (0..threads)
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
        .collect();

    // generate and send transactions for the specified duration
    let start = Instant::now();
    let keypair_chunks = source_keypair_chunks.len();
    let mut reclaim_lamports_back_to_source_account = false;
    let mut chunk_index = 0;
    while start.elapsed() < duration {
        generate_txs(
            &shared_txs,
            &recent_blockhash,
            &source_keypair_chunks[chunk_index],
            &dest_keypair_chunks[chunk_index],
            threads,
            reclaim_lamports_back_to_source_account,
            &libra_args,
        );

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

        // Move on to next chunk
        chunk_index = (chunk_index + 1) % keypair_chunks;

        // Switch directions after transfering for each "chunk"
        if chunk_index == 0 {
            reclaim_lamports_back_to_source_account = !reclaim_lamports_back_to_source_account;
        }
    }

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

    info!("Waiting for blockhash thread...");
    if let Err(err) = blockhash_thread.join() {
        info!("  join() failed with: {:?}", err);
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

#[cfg(feature = "move")]
fn generate_move_txs(
    source: &[&Keypair],
    dest: &VecDeque<&Keypair>,
    reclaim: bool,
    move_keypairs: &[Keypair],
    libra_pay_program_id: &Pubkey,
    libra_mint_id: &Pubkey,
    blockhash: &Hash,
) -> Vec<(Transaction, u64)> {
    let count = move_keypairs.len() / 2;
    let source_move = &move_keypairs[..count];
    let dest_move = &move_keypairs[count..];
    let pairs: Vec<_> = if !reclaim {
        source_move
            .iter()
            .zip(dest_move.iter())
            .zip(source.iter())
            .collect()
    } else {
        dest_move
            .iter()
            .zip(source_move.iter())
            .zip(dest.iter())
            .collect()
    };

    pairs
        .par_iter()
        .map(|((from, to), payer)| {
            (
                librapay_transaction::transfer(
                    libra_pay_program_id,
                    libra_mint_id,
                    &payer,
                    &from,
                    &to.pubkey(),
                    1,
                    *blockhash,
                ),
                timestamp(),
            )
        })
        .collect()
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

fn generate_txs(
    shared_txs: &SharedTransactions,
    blockhash: &Arc<RwLock<Hash>>,
    source: &[&Keypair],
    dest: &VecDeque<&Keypair>,
    threads: usize,
    reclaim: bool,
    libra_args: &Option<LibraKeys>,
) {
    let blockhash = *blockhash.read().unwrap();
    let tx_count = source.len();
    info!(
        "Signing transactions... {} (reclaim={}, blockhash={})",
        tx_count, reclaim, &blockhash
    );
    let signing_start = Instant::now();

    let transactions = if let Some((
        _libra_genesis_keypair,
        _libra_pay_program_id,
        _libra_mint_program_id,
        _libra_keys,
    )) = libra_args
    {
        #[cfg(not(feature = "move"))]
        {
            return;
        }

        #[cfg(feature = "move")]
        {
            generate_move_txs(
                source,
                dest,
                reclaim,
                &_libra_keys,
                _libra_pay_program_id,
                &_libra_genesis_keypair.pubkey(),
                &blockhash,
            )
        }
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

fn poll_blockhash<T: Client>(
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
            if let Ok((new_blockhash, _fee)) = client.get_new_blockhash(&old_blockhash) {
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
        }

        if exit_signal.load(Ordering::Relaxed) {
            break;
        }

        sleep(Duration::from_millis(50));
    }
}

fn do_tx_transfers<T: Client>(
    exit_signal: &Arc<AtomicBool>,
    shared_txs: &SharedTransactions,
    shared_tx_thread_count: &Arc<AtomicIsize>,
    total_tx_sent_count: &Arc<AtomicUsize>,
    thread_batch_sleep_ms: usize,
    client: &Arc<T>,
) {
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
            info!(
                "Transferring 1 unit {} times... to {}",
                txs0.len(),
                client.as_ref().tpu_addr(),
            );
            let tx_len = txs0.len();
            let transfer_start = Instant::now();
            let mut old_transactions = false;
            for tx in txs0 {
                let now = timestamp();
                // Transactions that are too old will be rejected by the cluster Don't bother
                // sending them.
                if now > tx.1 && now - tx.1 > 1000 * MAX_TX_QUEUE_AGE {
                    old_transactions = true;
                    continue;
                }
                client
                    .async_send_transaction(tx.0)
                    .expect("async_send_transaction in do_tx_transfers");
            }
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

fn verify_funding_transfer<T: Client>(client: &Arc<T>, tx: &Transaction, amount: u64) -> bool {
    for a in &tx.message().account_keys[1..] {
        match client.get_balance_with_commitment(a, CommitmentConfig::recent()) {
            Ok(balance) => return balance >= amount,
            Err(err) => error!("failed to get balance {:?}", err),
        }
    }
    false
}

trait FundingTransactions<'a> {
    fn fund<T: 'static + Client + Send + Sync>(
        &mut self,
        client: &Arc<T>,
        to_fund: &[(&'a Keypair, Vec<(Pubkey, u64)>)],
        to_lamports: u64,
    );
    fn make(&mut self, to_fund: &[(&'a Keypair, Vec<(Pubkey, u64)>)]);
    fn sign(&mut self, blockhash: Hash);
    fn send<T: Client>(&self, client: &Arc<T>);
    fn verify<T: 'static + Client + Send + Sync>(&mut self, client: &Arc<T>, to_lamports: u64);
}

impl<'a> FundingTransactions<'a> for Vec<(&'a Keypair, Transaction)> {
    fn fund<T: 'static + Client + Send + Sync>(
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

            let (blockhash, _fee_calculator) = get_recent_blockhash(client.as_ref());

            // re-sign retained to_fund_txes with updated blockhash
            self.sign(blockhash);
            self.send(&client);

            // Sleep a few slots to allow transactions to process
            sleep(Duration::from_secs(1));

            self.verify(&client, to_lamports);

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
                let tx = Transaction::new_unsigned_instructions(
                    &system_instruction::transfer_many(&k.pubkey(), &t),
                );
                (*k, tx)
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

    fn send<T: Client>(&self, client: &Arc<T>) {
        let mut send_txs = Measure::start("send_txs");
        self.iter().for_each(|(_, tx)| {
            client.async_send_transaction(tx.clone()).expect("transfer");
        });
        send_txs.stop();
        debug!("send {} txs: {}us", self.len(), send_txs.as_us());
    }

    fn verify<T: 'static + Client + Send + Sync>(&mut self, client: &Arc<T>, to_lamports: u64) {
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

                    let verified = if verify_funding_transfer(&client, &tx, to_lamports) {
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
pub fn fund_keys<T: 'static + Client + Send + Sync>(
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

pub fn airdrop_lamports<T: Client>(
    client: &T,
    faucet_addr: &SocketAddr,
    id: &Keypair,
    desired_balance: u64,
) -> Result<()> {
    let starting_balance = client.get_balance(&id.pubkey()).unwrap_or(0);
    metrics_submit_lamport_balance(starting_balance);
    info!("starting balance {}", starting_balance);

    if starting_balance < desired_balance {
        let airdrop_amount = desired_balance - starting_balance;
        info!(
            "Airdropping {:?} lamports from {} for {}",
            airdrop_amount,
            faucet_addr,
            id.pubkey(),
        );

        let (blockhash, _fee_calculator) = get_recent_blockhash(client);
        match request_airdrop_transaction(&faucet_addr, &id.pubkey(), airdrop_amount, blockhash) {
            Ok(transaction) => {
                let mut tries = 0;
                loop {
                    tries += 1;
                    let signature = client.async_send_transaction(transaction.clone()).unwrap();
                    let result = client.poll_for_signature_confirmation(&signature, 1);

                    if result.is_ok() {
                        break;
                    }
                    if tries >= 5 {
                        panic!(
                            "Error requesting airdrop: to addr: {:?} amount: {} {:?}",
                            faucet_addr, airdrop_amount, result
                        )
                    }
                }
            }
            Err(err) => {
                panic!(
                    "Error requesting airdrop: {:?} to addr: {:?} amount: {}",
                    err, faucet_addr, airdrop_amount
                );
            }
        };

        let current_balance = client
            .get_balance_with_commitment(&id.pubkey(), CommitmentConfig::recent())
            .unwrap_or_else(|e| {
                info!("airdrop error {}", e);
                starting_balance
            });
        info!("current balance {}...", current_balance);

        metrics_submit_lamport_balance(current_balance);
        if current_balance - starting_balance != airdrop_amount {
            info!(
                "Airdrop failed! {} {} {}",
                id.pubkey(),
                current_balance,
                starting_balance
            );
            return Err(BenchTpsError::AirdropFailure);
        }
    }
    Ok(())
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

#[cfg(feature = "move")]
fn fund_move_keys<T: Client>(
    client: &T,
    funding_key: &Keypair,
    keypairs: &[Keypair],
    total: u64,
    libra_pay_program_id: &Pubkey,
    libra_mint_program_id: &Pubkey,
    libra_genesis_key: &Keypair,
) {
    let (mut blockhash, _fee_calculator) = get_recent_blockhash(client);

    info!("creating the libra funding account..");
    let libra_funding_key = Keypair::new();
    let tx = librapay_transaction::create_account(funding_key, &libra_funding_key, 1, blockhash);
    client
        .send_message(&[funding_key, &libra_funding_key], tx.message)
        .unwrap();

    info!("minting to funding keypair");
    let tx = librapay_transaction::mint_tokens(
        &libra_mint_program_id,
        funding_key,
        libra_genesis_key,
        &libra_funding_key.pubkey(),
        total,
        blockhash,
    );
    client
        .send_message(&[funding_key, libra_genesis_key], tx.message)
        .unwrap();

    info!("creating {} move accounts...", keypairs.len());
    let total_len = keypairs.len();
    let create_len = 5;
    let mut funding_time = Measure::start("funding_time");
    for (i, keys) in keypairs.chunks(create_len).enumerate() {
        if client
            .get_balance_with_commitment(&keys[0].pubkey(), CommitmentConfig::recent())
            .unwrap_or(0)
            > 0
        {
            // already created these accounts.
            break;
        }

        let keypairs: Vec<_> = keys.iter().map(|k| k).collect();
        let tx = librapay_transaction::create_accounts(funding_key, &keypairs, 1, blockhash);
        let ser_size = bincode::serialized_size(&tx).unwrap();
        let mut keys = vec![funding_key];
        keys.extend(&keypairs);
        client.send_message(&keys, tx.message).unwrap();

        if i % 10 == 0 {
            info!(
                "created {} accounts of {} (size {})",
                i,
                total_len / create_len,
                ser_size,
            );
        }
    }

    const NUM_FUNDING_KEYS: usize = 10;
    let funding_keys: Vec<_> = (0..NUM_FUNDING_KEYS).map(|_| Keypair::new()).collect();
    let pubkey_amounts: Vec<_> = funding_keys
        .iter()
        .map(|key| (key.pubkey(), total / NUM_FUNDING_KEYS as u64))
        .collect();
    let tx = Transaction::new_signed_instructions(
        &[funding_key],
        &system_instruction::transfer_many(&funding_key.pubkey(), &pubkey_amounts),
        blockhash,
    );
    client.send_message(&[funding_key], tx.message).unwrap();
    let mut balance = 0;
    for _ in 0..20 {
        if let Ok(balance_) = client
            .get_balance_with_commitment(&funding_keys[0].pubkey(), CommitmentConfig::recent())
        {
            if balance_ > 0 {
                balance = balance_;
                break;
            }
        }
        sleep(Duration::from_millis(100));
    }
    assert!(balance > 0);
    info!(
        "funded multiple funding accounts with {:?} lanports",
        balance
    );

    let libra_funding_keys: Vec<_> = (0..NUM_FUNDING_KEYS).map(|_| Keypair::new()).collect();
    for (i, key) in libra_funding_keys.iter().enumerate() {
        let tx = librapay_transaction::create_account(&funding_keys[i], &key, 1, blockhash);
        client
            .send_message(&[&funding_keys[i], &key], tx.message)
            .unwrap();

        let tx = librapay_transaction::transfer(
            libra_pay_program_id,
            &libra_genesis_key.pubkey(),
            &funding_keys[i],
            &libra_funding_key,
            &key.pubkey(),
            total / NUM_FUNDING_KEYS as u64,
            blockhash,
        );
        client
            .send_message(&[&funding_keys[i], &libra_funding_key], tx.message)
            .unwrap();

        info!("funded libra funding key {}", i);
    }

    let keypair_count = keypairs.len();
    let amount = total / (keypair_count as u64);
    for (i, keys) in keypairs[..keypair_count]
        .chunks(NUM_FUNDING_KEYS)
        .enumerate()
    {
        for (j, key) in keys.iter().enumerate() {
            let tx = librapay_transaction::transfer(
                libra_pay_program_id,
                &libra_genesis_key.pubkey(),
                &funding_keys[j],
                &libra_funding_keys[j],
                &key.pubkey(),
                amount,
                blockhash,
            );

            let _sig = client
                .async_send_transaction(tx.clone())
                .expect("create_account in generate_and_fund_keypairs");
        }

        for (j, key) in keys.iter().enumerate() {
            let mut times = 0;
            loop {
                let balance =
                    librapay_transaction::get_libra_balance(client, &key.pubkey()).unwrap();
                if balance >= amount {
                    break;
                } else if times > 20 {
                    info!("timed out.. {} key: {} balance: {}", i, j, balance);
                    break;
                } else {
                    times += 1;
                    sleep(Duration::from_millis(100));
                }
            }
        }

        info!(
            "funded group {} of {}",
            i + 1,
            keypairs.len() / NUM_FUNDING_KEYS
        );
        blockhash = get_recent_blockhash(client).0;
    }

    funding_time.stop();
    info!("done funding keys, took {} ms", funding_time.as_ms());
}

pub fn generate_and_fund_keypairs<T: 'static + Client + Send + Sync>(
    client: Arc<T>,
    faucet_addr: Option<SocketAddr>,
    funding_key: &Keypair,
    keypair_count: usize,
    lamports_per_account: u64,
    use_move: bool,
) -> Result<(Vec<Keypair>, Option<LibraKeys>)> {
    info!("Creating {} keypairs...", keypair_count);
    let (mut keypairs, extra) = generate_keypairs(funding_key, keypair_count as u64);
    info!("Get lamports...");

    // Sample the first keypair, to prevent lamport loss on repeated solana-bench-tps executions
    let first_key = keypairs[0].pubkey();
    let first_keypair_balance = client.get_balance(&first_key).unwrap_or(0);

    // Sample the last keypair, to check if funding was already completed
    let last_key = keypairs[keypair_count - 1].pubkey();
    let last_keypair_balance = client.get_balance(&last_key).unwrap_or(0);

    #[cfg(feature = "move")]
    let mut move_keypairs_ret = None;

    #[cfg(not(feature = "move"))]
    let move_keypairs_ret = None;

    // Repeated runs will eat up keypair balances from transaction fees. In order to quickly
    //   start another bench-tps run without re-funding all of the keypairs, check if the
    //   keypairs still have at least 80% of the expected funds. That should be enough to
    //   pay for the transaction fees in a new run.
    let enough_lamports = 8 * lamports_per_account / 10;
    if first_keypair_balance < enough_lamports || last_keypair_balance < enough_lamports {
        let fee_rate_governor = client.get_fee_rate_governor().unwrap();
        let max_fee = fee_rate_governor.max_lamports_per_signature;
        let extra_fees = extra * max_fee;
        let total_keypairs = keypairs.len() as u64 + 1; // Add one for funding keypair
        let mut total = lamports_per_account * total_keypairs + extra_fees;
        if use_move {
            total *= 3;
        }

        let funding_key_balance = client.get_balance(&funding_key.pubkey()).unwrap_or(0);
        info!(
            "Funding keypair balance: {} max_fee: {} lamports_per_account: {} extra: {} total: {}",
            funding_key_balance, max_fee, lamports_per_account, extra, total
        );

        if client.get_balance(&funding_key.pubkey()).unwrap_or(0) < total {
            airdrop_lamports(client.as_ref(), &faucet_addr.unwrap(), funding_key, total)?;
        }

        #[cfg(feature = "move")]
        {
            if use_move {
                let libra_genesis_keypair =
                    create_genesis(&funding_key, client.as_ref(), 10_000_000);
                let libra_mint_program_id = upload_mint_script(&funding_key, client.as_ref());
                let libra_pay_program_id = upload_payment_script(&funding_key, client.as_ref());

                // Generate another set of keypairs for move accounts.
                // Still fund the solana ones which will be used for fees.
                let seed = [0u8; 32];
                let mut rnd = GenKeys::new(seed);
                let move_keypairs = rnd.gen_n_keypairs(keypair_count as u64);
                fund_move_keys(
                    client.as_ref(),
                    funding_key,
                    &move_keypairs,
                    total / 3,
                    &libra_pay_program_id,
                    &libra_mint_program_id,
                    &libra_genesis_keypair,
                );
                move_keypairs_ret = Some((
                    libra_genesis_keypair,
                    libra_pay_program_id,
                    libra_mint_program_id,
                    move_keypairs,
                ));

                // Give solana keys 1/3 and move keys 1/3 the lamports. Keep 1/3 for fees.
                total /= 3;
            }
        }

        fund_keys(
            client,
            funding_key,
            &keypairs,
            total,
            max_fee,
            lamports_per_account,
        );
    }

    // 'generate_keypairs' generates extra keys to be able to have size-aligned funding batches for fund_keys.
    keypairs.truncate(keypair_count);

    Ok((keypairs, move_keypairs_ret))
}

#[cfg(test)]
mod tests {
    use super::*;
    use solana_runtime::bank::Bank;
    use solana_runtime::bank_client::BankClient;
    use solana_sdk::client::SyncClient;
    use solana_sdk::fee_calculator::FeeRateGovernor;
    use solana_sdk::genesis_config::create_genesis_config;

    #[test]
    fn test_bench_tps_bank_client() {
        let (genesis_config, id) = create_genesis_config(10_000);
        let bank = Bank::new(&genesis_config);
        let client = Arc::new(BankClient::new(bank));

        let mut config = Config::default();
        config.id = id;
        config.tx_count = 10;
        config.duration = Duration::from_secs(5);

        let keypair_count = config.tx_count * config.keypair_multiplier;
        let (keypairs, _move_keypairs) =
            generate_and_fund_keypairs(client.clone(), None, &config.id, keypair_count, 20, false)
                .unwrap();

        do_bench_tps(client, config, keypairs, None);
    }

    #[test]
    fn test_bench_tps_fund_keys() {
        let (genesis_config, id) = create_genesis_config(10_000);
        let bank = Bank::new(&genesis_config);
        let client = Arc::new(BankClient::new(bank));
        let keypair_count = 20;
        let lamports = 20;

        let (keypairs, _move_keypairs) =
            generate_and_fund_keypairs(client.clone(), None, &id, keypair_count, lamports, false)
                .unwrap();

        for kp in &keypairs {
            assert_eq!(
                client
                    .get_balance_with_commitment(&kp.pubkey(), CommitmentConfig::recent())
                    .unwrap(),
                lamports
            );
        }
    }

    #[test]
    fn test_bench_tps_fund_keys_with_fees() {
        let (mut genesis_config, id) = create_genesis_config(10_000);
        let fee_rate_governor = FeeRateGovernor::new(11, 0);
        genesis_config.fee_rate_governor = fee_rate_governor;
        let bank = Bank::new(&genesis_config);
        let client = Arc::new(BankClient::new(bank));
        let keypair_count = 20;
        let lamports = 20;

        let (keypairs, _move_keypairs) =
            generate_and_fund_keypairs(client.clone(), None, &id, keypair_count, lamports, false)
                .unwrap();

        for kp in &keypairs {
            assert_eq!(client.get_balance(&kp.pubkey()).unwrap(), lamports);
        }
    }
}
