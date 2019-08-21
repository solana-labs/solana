use solana_metrics;

use bincode;
use log::*;
use rayon::prelude::*;
use solana_client::perf_utils::{sample_txs, SampleStats};
use solana_core::gen_keys::GenKeys;
use solana_drone::drone::request_airdrop_transaction;
use solana_librapay_api::{create_genesis, upload_mint_program, upload_payment_program};
use solana_measure::measure::Measure;
use solana_metrics::datapoint_info;
use solana_sdk::client::Client;
use solana_sdk::hash::Hash;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Keypair, KeypairUtil};
use solana_sdk::system_instruction;
use solana_sdk::system_transaction;
use solana_sdk::timing::timestamp;
use solana_sdk::timing::{duration_as_ms, duration_as_s};
use solana_sdk::transaction::Transaction;
use std::cmp;
use std::collections::VecDeque;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicUsize, Ordering};
use std::sync::{Arc, RwLock};
use std::thread::sleep;
use std::thread::Builder;
use std::time::Duration;
use std::time::Instant;

use solana_librapay_api::librapay_transaction;

pub const MAX_SPENDS_PER_TX: u64 = 4;
pub const NUM_LAMPORTS_PER_ACCOUNT: u64 = 128;

#[derive(Debug)]
pub enum BenchTpsError {
    AirdropFailure,
}

pub type Result<T> = std::result::Result<T, BenchTpsError>;

pub type SharedTransactions = Arc<RwLock<VecDeque<Vec<(Transaction, u64)>>>>;

pub struct Config {
    pub id: Keypair,
    pub threads: usize,
    pub thread_batch_sleep_ms: usize,
    pub duration: Duration,
    pub tx_count: usize,
    pub sustained: bool,
    pub use_move: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            id: Keypair::new(),
            threads: 4,
            thread_batch_sleep_ms: 0,
            duration: Duration::new(std::u64::MAX, 0),
            tx_count: 500_000,
            sustained: false,
            use_move: false,
        }
    }
}

type LibraKeys = (Keypair, Pubkey, Pubkey, Vec<Keypair>);

pub fn do_bench_tps<T>(
    clients: Vec<T>,
    config: Config,
    gen_keypairs: Vec<Keypair>,
    keypair0_balance: u64,
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

    let clients: Vec<_> = clients.into_iter().map(Arc::new).collect();
    let client = &clients[0];

    let start = gen_keypairs.len() - (tx_count * 2) as usize;
    let keypairs = &gen_keypairs[start..];

    let first_tx_count = client.get_transaction_count().expect("transaction count");
    println!("Initial transaction count {}", first_tx_count);

    let exit_signal = Arc::new(AtomicBool::new(false));

    // Setup a thread per validator to sample every period
    // collect the max transaction rate and total tx count seen
    let maxes = Arc::new(RwLock::new(Vec::new()));
    let sample_period = 1; // in seconds
    println!("Sampling TPS every {} second...", sample_period);
    let v_threads: Vec<_> = clients
        .iter()
        .map(|client| {
            let exit_signal = exit_signal.clone();
            let maxes = maxes.clone();
            let client = client.clone();
            Builder::new()
                .name("solana-client-sample".to_string())
                .spawn(move || {
                    sample_txs(&exit_signal, &maxes, sample_period, &client);
                })
                .unwrap()
        })
        .collect();

    let shared_txs: SharedTransactions = Arc::new(RwLock::new(VecDeque::new()));

    let shared_tx_active_thread_count = Arc::new(AtomicIsize::new(0));
    let total_tx_sent_count = Arc::new(AtomicUsize::new(0));

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
    let mut reclaim_lamports_back_to_source_account = false;
    let mut i = keypair0_balance;
    let mut blockhash = Hash::default();
    let mut blockhash_time = Instant::now();
    while start.elapsed() < duration {
        // ping-pong between source and destination accounts for each loop iteration
        // this seems to be faster than trying to determine the balance of individual
        // accounts
        let len = tx_count as usize;
        if let Ok((new_blockhash, _fee_calculator)) = client.get_new_blockhash(&blockhash) {
            blockhash = new_blockhash;
        } else {
            if blockhash_time.elapsed().as_secs() > 30 {
                panic!("Blockhash is not updating");
            }
            sleep(Duration::from_millis(100));
            continue;
        }
        blockhash_time = Instant::now();
        let balance = client.get_balance(&id.pubkey()).unwrap_or(0);
        metrics_submit_lamport_balance(balance);
        generate_txs(
            &shared_txs,
            &blockhash,
            &keypairs[..len],
            &keypairs[len..],
            threads,
            reclaim_lamports_back_to_source_account,
            &libra_args,
        );
        // In sustained mode overlap the transfers with generation
        // this has higher average performance but lower peak performance
        // in tested environments.
        if !sustained {
            while shared_tx_active_thread_count.load(Ordering::Relaxed) > 0 {
                sleep(Duration::from_millis(1));
            }
        }

        i += 1;
        if should_switch_directions(NUM_LAMPORTS_PER_ACCOUNT, i) {
            reclaim_lamports_back_to_source_account = !reclaim_lamports_back_to_source_account;
        }
    }

    // Stop the sampling threads so it will collect the stats
    exit_signal.store(true, Ordering::Relaxed);

    println!("Waiting for validator threads...");
    for t in v_threads {
        if let Err(err) = t.join() {
            println!("  join() failed with: {:?}", err);
        }
    }

    // join the tx send threads
    println!("Waiting for transmit threads...");
    for t in s_threads {
        if let Err(err) = t.join() {
            println!("  join() failed with: {:?}", err);
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
    println!("Token balance: {}", lamport_balance);
    datapoint_info!(
        "bench-tps-lamport_balance",
        ("balance", lamport_balance, i64)
    );
}

fn generate_move_txs(
    source: &[Keypair],
    dest: &[Keypair],
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
    source: &[Keypair],
    dest: &[Keypair],
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
                system_transaction::create_user_account(from, &to.pubkey(), 1, *blockhash),
                timestamp(),
            )
        })
        .collect()
}

fn generate_txs(
    shared_txs: &SharedTransactions,
    blockhash: &Hash,
    source: &[Keypair],
    dest: &[Keypair],
    threads: usize,
    reclaim: bool,
    libra_args: &Option<LibraKeys>,
) {
    let tx_count = source.len();
    println!("Signing transactions... {} (reclaim={})", tx_count, reclaim);
    let signing_start = Instant::now();

    let transactions = if let Some((
        libra_genesis_keypair,
        libra_pay_program_id,
        _libra_mint_program_id,
        libra_keys,
    )) = libra_args
    {
        generate_move_txs(
            source,
            dest,
            reclaim,
            &libra_keys,
            libra_pay_program_id,
            &libra_genesis_keypair.pubkey(),
            blockhash,
        )
    } else {
        generate_system_txs(source, dest, reclaim, blockhash)
    };

    let duration = signing_start.elapsed();
    let ns = duration.as_secs() * 1_000_000_000 + u64::from(duration.subsec_nanos());
    let bsps = (tx_count) as f64 / ns as f64;
    let nsps = ns as f64 / (tx_count) as f64;
    println!(
        "Done. {:.2} thousand signatures per second, {:.2} us per signature, {} ms total time, {}",
        bsps * 1_000_000_f64,
        nsps / 1_000_f64,
        duration_as_ms(&duration),
        blockhash,
    );
    datapoint_info!(
        "bench-tps-generate_txs",
        ("duration", duration_as_ms(&duration), i64)
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
        let txs;
        {
            let mut shared_txs_wl = shared_txs.write().expect("write lock in do_tx_transfers");
            txs = shared_txs_wl.pop_front();
        }
        if let Some(txs0) = txs {
            shared_tx_thread_count.fetch_add(1, Ordering::Relaxed);
            println!(
                "Transferring 1 unit {} times... to {}",
                txs0.len(),
                client.as_ref().transactions_addr(),
            );
            let tx_len = txs0.len();
            let transfer_start = Instant::now();
            for tx in txs0 {
                let now = timestamp();
                if now > tx.1 && now - tx.1 > 1000 * 30 {
                    continue;
                }
                client
                    .async_send_transaction(tx.0)
                    .expect("async_send_transaction in do_tx_transfers");
            }
            shared_tx_thread_count.fetch_add(-1, Ordering::Relaxed);
            total_tx_sent_count.fetch_add(tx_len, Ordering::Relaxed);
            println!(
                "Tx send done. {} ms {} tps",
                duration_as_ms(&transfer_start.elapsed()),
                tx_len as f32 / duration_as_s(&transfer_start.elapsed()),
            );
            datapoint_info!(
                "bench-tps-do_tx_transfers",
                ("duration", duration_as_ms(&transfer_start.elapsed()), i64),
                ("count", tx_len, i64)
            );
        }
        if exit_signal.load(Ordering::Relaxed) {
            break;
        }
    }
}

fn verify_funding_transfer<T: Client>(client: &T, tx: &Transaction, amount: u64) -> bool {
    for a in &tx.message().account_keys[1..] {
        if client.get_balance(a).unwrap_or(0) >= amount {
            return true;
        }
    }

    false
}

/// fund the dests keys by spending all of the source keys into MAX_SPENDS_PER_TX
/// on every iteration.  This allows us to replay the transfers because the source is either empty,
/// or full
pub fn fund_keys<T: Client>(
    client: &T,
    source: &Keypair,
    dests: &[Keypair],
    total: u64,
    max_fee: u64,
    mut extra: u64,
) {
    let mut funded: Vec<(&Keypair, u64)> = vec![(source, total)];
    let mut notfunded: Vec<&Keypair> = dests.iter().collect();
    let lamports_per_account = (total - (extra * max_fee)) / (notfunded.len() as u64 + 1);

    println!(
        "funding keys {} with lamports: {:?} total: {}",
        dests.len(),
        client.get_balance(&source.pubkey()),
        total
    );
    while !notfunded.is_empty() {
        let mut new_funded: Vec<(&Keypair, u64)> = vec![];
        let mut to_fund = vec![];
        println!("creating from... {}", funded.len());
        for f in &mut funded {
            let max_units = cmp::min(notfunded.len() as u64, MAX_SPENDS_PER_TX);
            if max_units == 0 {
                break;
            }
            let start = notfunded.len() - max_units as usize;
            let fees = if extra > 0 { max_fee } else { 0 };
            let per_unit = (f.1 - lamports_per_account - fees) / max_units;
            let moves: Vec<_> = notfunded[start..]
                .iter()
                .map(|k| (k.pubkey(), per_unit))
                .collect();
            notfunded[start..]
                .iter()
                .for_each(|k| new_funded.push((k, per_unit)));
            notfunded.truncate(start);
            if !moves.is_empty() {
                to_fund.push((f.0, moves));
            }
            extra -= 1;
        }

        // try to transfer a "few" at a time with recent blockhash
        //  assume 4MB network buffers, and 512 byte packets
        const FUND_CHUNK_LEN: usize = 4 * 1024 * 1024 / 512;

        to_fund.chunks(FUND_CHUNK_LEN).for_each(|chunk| {
            let mut tries = 0;

            // this set of transactions just initializes us for bookkeeping
            #[allow(clippy::clone_double_ref)] // sigh
            let mut to_fund_txs: Vec<_> = chunk
                .par_iter()
                .map(|(k, m)| {
                    let tx = Transaction::new_unsigned_instructions(
                        system_instruction::transfer_many(&k.pubkey(), &m),
                    );
                    (k.clone(), tx)
                })
                .collect();

            let amount = chunk[0].1[0].1;

            while !to_fund_txs.is_empty() {
                let receivers = to_fund_txs
                    .iter()
                    .fold(0, |len, (_, tx)| len + tx.message().instructions.len());

                println!(
                    "{} {} to {} in {} txs",
                    if tries == 0 {
                        "transferring"
                    } else {
                        " retrying"
                    },
                    amount,
                    receivers,
                    to_fund_txs.len(),
                );

                let (blockhash, _fee_calculator) = client.get_recent_blockhash().unwrap();

                // re-sign retained to_fund_txes with updated blockhash
                to_fund_txs.par_iter_mut().for_each(|(k, tx)| {
                    tx.sign(&[*k], blockhash);
                });

                to_fund_txs.iter().for_each(|(_, tx)| {
                    client.async_send_transaction(tx.clone()).expect("transfer");
                });

                // retry anything that seems to have dropped through cracks
                //  again since these txs are all or nothing, they're fine to
                //  retry
                for _ in 0..10 {
                    to_fund_txs.retain(|(_, tx)| !verify_funding_transfer(client, &tx, amount));
                    if to_fund_txs.is_empty() {
                        break;
                    }
                    sleep(Duration::from_millis(100));
                }

                tries += 1;
            }
            println!("transferred");
        });
        println!("funded: {} left: {}", new_funded.len(), notfunded.len());
        funded = new_funded;
    }
}

pub fn airdrop_lamports<T: Client>(
    client: &T,
    drone_addr: &SocketAddr,
    id: &Keypair,
    tx_count: u64,
) -> Result<()> {
    let starting_balance = client.get_balance(&id.pubkey()).unwrap_or(0);
    metrics_submit_lamport_balance(starting_balance);
    println!("starting balance {}", starting_balance);

    if starting_balance < tx_count {
        let airdrop_amount = tx_count - starting_balance;
        println!(
            "Airdropping {:?} lamports from {} for {}",
            airdrop_amount,
            drone_addr,
            id.pubkey(),
        );

        let (blockhash, _fee_calculator) = client.get_recent_blockhash().unwrap();
        match request_airdrop_transaction(&drone_addr, &id.pubkey(), airdrop_amount, blockhash) {
            Ok(transaction) => {
                let signature = client.async_send_transaction(transaction).unwrap();
                client
                    .poll_for_signature_confirmation(&signature, 1)
                    .unwrap_or_else(|_| {
                        panic!(
                            "Error requesting airdrop: to addr: {:?} amount: {}",
                            drone_addr, airdrop_amount
                        )
                    })
            }
            Err(err) => {
                panic!(
                    "Error requesting airdrop: {:?} to addr: {:?} amount: {}",
                    err, drone_addr, airdrop_amount
                );
            }
        };

        let current_balance = client.get_balance(&id.pubkey()).unwrap_or_else(|e| {
            println!("airdrop error {}", e);
            starting_balance
        });
        println!("current balance {}...", current_balance);

        metrics_submit_lamport_balance(current_balance);
        if current_balance - starting_balance != airdrop_amount {
            println!(
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
    println!(" Node address        |       Max TPS | Total Transactions");
    println!("---------------------+---------------+--------------------");

    for (sock, stats) in maxes.read().unwrap().iter() {
        let maybe_flag = match stats.txs {
            0 => "!!!!!",
            _ => "",
        };

        println!(
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
        println!(
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
    println!(
        "\nHighest TPS: {:.2} sampling period {}s max transactions: {} clients: {} drop rate: {:.2}",
        max_of_maxes,
        sample_period,
        max_tx_count,
        maxes.read().unwrap().len(),
        drop_rate,
    );
    println!(
        "\tAverage TPS: {}",
        max_tx_count as f32 / duration_as_s(tx_send_elapsed)
    );
}

// First transfer 3/4 of the lamports to the dest accounts
// then ping-pong 1/4 of the lamports back to the other account
// this leaves 1/4 lamport buffer in each account
fn should_switch_directions(num_lamports_per_account: u64, i: u64) -> bool {
    i % (num_lamports_per_account / 4) == 0 && (i >= (3 * num_lamports_per_account) / 4)
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

fn fund_move_keys<T: Client>(
    client: &T,
    funding_key: &Keypair,
    keypairs: &[Keypair],
    total: u64,
    libra_pay_program_id: &Pubkey,
    libra_mint_program_id: &Pubkey,
    libra_mint_key: &Keypair,
) {
    let (mut blockhash, _fee_calculator) = client.get_recent_blockhash().unwrap();

    info!("creating the libra funding account..");
    let libra_funding_key = Keypair::new();
    let tx = librapay_transaction::create_account(
        funding_key,
        &libra_funding_key.pubkey(),
        1,
        blockhash,
    );
    client.send_message(&[funding_key], tx.message).unwrap();

    info!("minting to funding keypair");
    let tx = librapay_transaction::mint_tokens(
        &libra_mint_program_id,
        funding_key,
        libra_mint_key,
        &libra_funding_key.pubkey(),
        total,
        blockhash,
    );
    client
        .send_message(&[funding_key, libra_mint_key], tx.message)
        .unwrap();

    info!("creating {} move accounts...", keypairs.len());
    let create_len = 8;
    let mut funding_time = Measure::start("funding_time");
    for (i, keys) in keypairs.chunks(create_len).enumerate() {
        if client.get_balance(&keys[0].pubkey()).unwrap_or(0) > 0 {
            // already created these accounts.
            break;
        }

        let pubkeys: Vec<_> = keys.iter().map(|k| k.pubkey()).collect();
        let tx = librapay_transaction::create_accounts(funding_key, &pubkeys, 1, blockhash);

        let ser_size = bincode::serialized_size(&tx).unwrap();

        client.send_message(&[funding_key], tx.message).unwrap();

        if i % 10 == 0 {
            info!(
                "size: {} created {} accounts of {}",
                ser_size,
                i,
                (keypairs.len() / create_len),
            );
        }
    }
    funding_time.stop();
    info!("funding accounts {}ms", funding_time.as_ms());

    const NUM_FUNDING_KEYS: usize = 4;
    let funding_keys: Vec<_> = (0..NUM_FUNDING_KEYS).map(|_| Keypair::new()).collect();
    let pubkey_amounts: Vec<_> = funding_keys
        .iter()
        .map(|key| (key.pubkey(), total / NUM_FUNDING_KEYS as u64))
        .collect();
    let tx = Transaction::new_signed_instructions(
        &[funding_key],
        system_instruction::transfer_many(&funding_key.pubkey(), &pubkey_amounts),
        blockhash,
    );
    client.send_message(&[funding_key], tx.message).unwrap();
    let mut balance = 0;
    for _ in 0..20 {
        balance = client.get_balance(&funding_keys[0].pubkey()).unwrap();
        if balance > 0 {
            break;
        } else {
            sleep(Duration::from_millis(100));
        }
    }
    assert!(balance > 0);
    info!("funded multiple funding accounts.. {:?}", balance);

    let libra_funding_keys: Vec<_> = (0..NUM_FUNDING_KEYS).map(|_| Keypair::new()).collect();
    for (i, key) in libra_funding_keys.iter().enumerate() {
        let tx =
            librapay_transaction::create_account(&funding_keys[i], &key.pubkey(), 1, blockhash);
        client
            .send_message(&[&funding_keys[i]], tx.message)
            .unwrap();

        let tx = librapay_transaction::transfer(
            libra_pay_program_id,
            &libra_mint_key.pubkey(),
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

    let tx_count = keypairs.len();
    let amount = total / (tx_count as u64);
    for (i, keys) in keypairs[..tx_count].chunks(NUM_FUNDING_KEYS).enumerate() {
        for (j, key) in keys.iter().enumerate() {
            let tx = librapay_transaction::transfer(
                libra_pay_program_id,
                &libra_mint_key.pubkey(),
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

        info!("sent... checking balance {}", i);
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

        info!("funded: {} of {}", i, keypairs.len() / NUM_FUNDING_KEYS);
        blockhash = client.get_recent_blockhash().unwrap().0;
    }

    info!("done funding keys..");
}

pub fn generate_and_fund_keypairs<T: Client>(
    client: &T,
    drone_addr: Option<SocketAddr>,
    funding_key: &Keypair,
    tx_count: usize,
    lamports_per_account: u64,
    use_move: bool,
) -> Result<(Vec<Keypair>, Option<LibraKeys>, u64)> {
    info!("Creating {} keypairs...", tx_count * 2);
    let (mut keypairs, extra) = generate_keypairs(funding_key, tx_count as u64 * 2);
    info!("Get lamports...");

    // Sample the first keypair, see if it has lamports, if so then resume.
    // This logic is to prevent lamport loss on repeated solana-bench-tps executions
    let last_keypair_balance = client
        .get_balance(&keypairs[tx_count * 2 - 1].pubkey())
        .unwrap_or(0);

    let mut move_keypairs_ret = None;

    if lamports_per_account > last_keypair_balance {
        let (_blockhash, fee_calculator) = client.get_recent_blockhash().unwrap();
        let account_desired_balance =
            lamports_per_account - last_keypair_balance + fee_calculator.max_lamports_per_signature;
        let extra_fees = extra * fee_calculator.max_lamports_per_signature;
        let mut total = account_desired_balance * (1 + keypairs.len() as u64) + extra_fees;
        if use_move {
            total *= 3;
        }

        println!("Previous key balance: {} max_fee: {} lamports_per_account: {} extra: {} desired_balance: {} total: {}",
                 last_keypair_balance, fee_calculator.max_lamports_per_signature, lamports_per_account, extra,
                 account_desired_balance, total
                 );

        if client.get_balance(&funding_key.pubkey()).unwrap_or(0) < total {
            airdrop_lamports(client, &drone_addr.unwrap(), funding_key, total)?;
        }

        if use_move {
            let libra_genesis_keypair = create_genesis(&funding_key, client, 10_000_000);
            let libra_mint_program_id = upload_mint_program(&funding_key, client);
            let libra_pay_program_id = upload_payment_program(&funding_key, client);

            // Generate another set of keypairs for move accounts.
            // Still fund the solana ones which will be used for fees.
            let seed = [0u8; 32];
            let mut rnd = GenKeys::new(seed);
            let move_keypairs = rnd.gen_n_keypairs(tx_count as u64 * 2);
            fund_move_keys(
                client,
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

        fund_keys(
            client,
            funding_key,
            &keypairs,
            total,
            fee_calculator.max_lamports_per_signature,
            extra,
        );
    }

    // 'generate_keypairs' generates extra keys to be able to have size-aligned funding batches for fund_keys.
    keypairs.truncate(2 * tx_count);

    Ok((keypairs, move_keypairs_ret, last_keypair_balance))
}

#[cfg(test)]
mod tests {

    use super::*;
    use solana_client::thin_client::create_client;
    use solana_core::cluster_info::FULLNODE_PORT_RANGE;
    use solana_core::validator::ValidatorConfig;
    use solana_drone::drone::run_local_drone;
    use solana_local_cluster::local_cluster::{ClusterConfig, LocalCluster};
    use solana_runtime::bank::Bank;
    use solana_runtime::bank_client::BankClient;
    use solana_sdk::client::SyncClient;
    use solana_sdk::fee_calculator::FeeCalculator;
    use solana_sdk::genesis_block::create_genesis_block;
    use std::sync::mpsc::channel;

    #[test]
    fn test_switch_directions() {
        assert_eq!(should_switch_directions(20, 0), false);
        assert_eq!(should_switch_directions(20, 1), false);
        assert_eq!(should_switch_directions(20, 14), false);
        assert_eq!(should_switch_directions(20, 15), true);
        assert_eq!(should_switch_directions(20, 16), false);
        assert_eq!(should_switch_directions(20, 19), false);
        assert_eq!(should_switch_directions(20, 20), true);
        assert_eq!(should_switch_directions(20, 21), false);
        assert_eq!(should_switch_directions(20, 99), false);
        assert_eq!(should_switch_directions(20, 100), true);
        assert_eq!(should_switch_directions(20, 101), false);
    }

    fn test_bench_tps_local_cluster(config: Config) {
        solana_logger::setup();
        const NUM_NODES: usize = 1;
        let cluster = LocalCluster::new(&ClusterConfig {
            node_stakes: vec![999_990; NUM_NODES],
            cluster_lamports: 200_000_000,
            validator_configs: vec![ValidatorConfig::default(); NUM_NODES],
            native_instruction_processors: vec![solana_move_loader_program!()],
            ..ClusterConfig::default()
        });

        let drone_keypair = Keypair::new();
        cluster.transfer(
            &cluster.funding_keypair,
            &drone_keypair.pubkey(),
            100_000_000,
        );

        let client = create_client(
            (cluster.entry_point_info.rpc, cluster.entry_point_info.tpu),
            FULLNODE_PORT_RANGE,
        );

        let (addr_sender, addr_receiver) = channel();
        run_local_drone(drone_keypair, addr_sender, None);
        let drone_addr = addr_receiver.recv_timeout(Duration::from_secs(2)).unwrap();

        let lamports_per_account = 100;

        let (keypairs, move_keypairs, _keypair_balance) = generate_and_fund_keypairs(
            &client,
            Some(drone_addr),
            &config.id,
            config.tx_count,
            lamports_per_account,
            config.use_move,
        )
        .unwrap();

        let total = do_bench_tps(vec![client], config, keypairs, 0, move_keypairs);
        assert!(total > 100);
    }

    #[test]
    fn test_bench_tps_local_cluster_solana() {
        let mut config = Config::default();
        config.tx_count = 100;
        config.duration = Duration::from_secs(10);

        test_bench_tps_local_cluster(config);
    }

    #[test]
    fn test_bench_tps_local_cluster_move() {
        let mut config = Config::default();
        config.tx_count = 100;
        config.duration = Duration::from_secs(20);
        config.use_move = true;

        test_bench_tps_local_cluster(config);
    }

    #[test]
    fn test_bench_tps_bank_client() {
        let (genesis_block, id) = create_genesis_block(10_000);
        let bank = Bank::new(&genesis_block);
        let clients = vec![BankClient::new(bank)];

        let mut config = Config::default();
        config.id = id;
        config.tx_count = 10;
        config.duration = Duration::from_secs(5);

        let (keypairs, _move_keypairs, _keypair_balance) =
            generate_and_fund_keypairs(&clients[0], None, &config.id, config.tx_count, 20, false)
                .unwrap();

        do_bench_tps(clients, config, keypairs, 0, None);
    }

    #[test]
    fn test_bench_tps_fund_keys() {
        let (genesis_block, id) = create_genesis_block(10_000);
        let bank = Bank::new(&genesis_block);
        let client = BankClient::new(bank);
        let tx_count = 10;
        let lamports = 20;

        let (keypairs, _move_keypairs, _keypair_balance) =
            generate_and_fund_keypairs(&client, None, &id, tx_count, lamports, false).unwrap();

        for kp in &keypairs {
            assert_eq!(client.get_balance(&kp.pubkey()).unwrap(), lamports);
        }
    }

    #[test]
    fn test_bench_tps_fund_keys_with_fees() {
        let (mut genesis_block, id) = create_genesis_block(10_000);
        let fee_calculator = FeeCalculator::new(11);
        genesis_block.fee_calculator = fee_calculator;
        let bank = Bank::new(&genesis_block);
        let client = BankClient::new(bank);
        let tx_count = 10;
        let lamports = 20;

        let (keypairs, _move_keypairs, _keypair_balance) =
            generate_and_fund_keypairs(&client, None, &id, tx_count, lamports, false).unwrap();

        let max_fee = client
            .get_recent_blockhash()
            .unwrap()
            .1
            .max_lamports_per_signature;
        for kp in &keypairs {
            assert_eq!(
                client.get_balance(&kp.pubkey()).unwrap(),
                lamports + max_fee
            );
        }
    }
}
