mod bench;
mod cli;

use crate::bench::*;
use solana::client::mk_client;
use solana::gen_keys::GenKeys;
use solana::gossip_service::discover;
use solana_metrics;
use solana_sdk::signature::{Keypair, KeypairUtil};
use std::collections::VecDeque;
use std::process::exit;
use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicUsize, Ordering};
use std::sync::{Arc, RwLock};
use std::thread::sleep;
use std::thread::Builder;
use std::time::Duration;
use std::time::Instant;

fn main() {
    solana_logger::setup();
    solana_metrics::set_panic_hook("bench-tps");

    let matches = cli::build_args().get_matches();

    let cfg = cli::extract_args(&matches);

    let cli::Config {
        network_addr: network,
        drone_addr,
        id,
        threads,
        thread_batch_sleep_ms,
        num_nodes,
        duration,
        tx_count,
        sustained,
        reject_extra_nodes,
        converge_only,
    } = cfg;

    let nodes = discover(&network, num_nodes).unwrap_or_else(|err| {
        eprintln!("Failed to discover {} nodes: {:?}", num_nodes, err);
        exit(1);
    });
    if nodes.len() < num_nodes {
        eprintln!(
            "Error: Insufficient nodes discovered.  Expecting {} or more",
            num_nodes
        );
        exit(1);
    }
    if reject_extra_nodes && nodes.len() > num_nodes {
        eprintln!(
            "Error: Extra nodes discovered.  Expecting exactly {}",
            num_nodes
        );
        exit(1);
    }

    if converge_only {
        return;
    }
    let cluster_entrypoint = nodes[0].clone(); // Pick the first node, why not?

    let mut client = mk_client(&cluster_entrypoint);
    let mut barrier_client = mk_client(&cluster_entrypoint);

    let mut seed = [0u8; 32];
    seed.copy_from_slice(&id.public_key_bytes()[..32]);
    let mut rnd = GenKeys::new(seed);

    println!("Creating {} keypairs...", tx_count * 2);
    let mut total_keys = 0;
    let mut target = tx_count * 2;
    while target > 0 {
        total_keys += target;
        target /= MAX_SPENDS_PER_TX;
    }
    let gen_keypairs = rnd.gen_n_keypairs(total_keys as u64);
    let barrier_source_keypair = Keypair::new();
    let barrier_dest_id = Keypair::new().pubkey();

    println!("Get lamports...");
    let num_lamports_per_account = 20;

    // Sample the first keypair, see if it has lamports, if so then resume
    // to avoid lamport loss
    let keypair0_balance = client
        .poll_get_balance(&gen_keypairs.last().unwrap().pubkey())
        .unwrap_or(0);

    if num_lamports_per_account > keypair0_balance {
        let extra = num_lamports_per_account - keypair0_balance;
        let total = extra * (gen_keypairs.len() as u64);
        airdrop_lamports(&mut client, &drone_addr, &id, total);
        println!("adding more lamports {}", extra);
        fund_keys(&mut client, &id, &gen_keypairs, extra);
    }
    let start = gen_keypairs.len() - (tx_count * 2) as usize;
    let keypairs = &gen_keypairs[start..];
    airdrop_lamports(&mut barrier_client, &drone_addr, &barrier_source_keypair, 1);

    println!("Get last ID...");
    let mut blockhash = client.get_recent_blockhash();
    println!("Got last ID {:?}", blockhash);

    let first_tx_count = client.transaction_count();
    println!("Initial transaction count {}", first_tx_count);

    let exit_signal = Arc::new(AtomicBool::new(false));

    // Setup a thread per validator to sample every period
    // collect the max transaction rate and total tx count seen
    let maxes = Arc::new(RwLock::new(Vec::new()));
    let sample_period = 1; // in seconds
    println!("Sampling TPS every {} second...", sample_period);
    let v_threads: Vec<_> = nodes
        .into_iter()
        .map(|v| {
            let exit_signal = exit_signal.clone();
            let maxes = maxes.clone();
            Builder::new()
                .name("solana-client-sample".to_string())
                .spawn(move || {
                    sample_tx_count(&exit_signal, &maxes, first_tx_count, &v, sample_period);
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
            let cluster_entrypoint = cluster_entrypoint.clone();
            let shared_tx_active_thread_count = shared_tx_active_thread_count.clone();
            let total_tx_sent_count = total_tx_sent_count.clone();
            Builder::new()
                .name("solana-client-sender".to_string())
                .spawn(move || {
                    do_tx_transfers(
                        &exit_signal,
                        &shared_txs,
                        &cluster_entrypoint,
                        &shared_tx_active_thread_count,
                        &total_tx_sent_count,
                        thread_batch_sleep_ms,
                    );
                })
                .unwrap()
        })
        .collect();

    // generate and send transactions for the specified duration
    let start = Instant::now();
    let mut reclaim_lamports_back_to_source_account = false;
    let mut i = keypair0_balance;
    while start.elapsed() < duration {
        let balance = client.poll_get_balance(&id.pubkey()).unwrap_or(0);
        metrics_submit_lamport_balance(balance);

        // ping-pong between source and destination accounts for each loop iteration
        // this seems to be faster than trying to determine the balance of individual
        // accounts
        let len = tx_count as usize;
        generate_txs(
            &shared_txs,
            &keypairs[..len],
            &keypairs[len..],
            threads,
            reclaim_lamports_back_to_source_account,
            &cluster_entrypoint,
        );
        // In sustained mode overlap the transfers with generation
        // this has higher average performance but lower peak performance
        // in tested environments.
        if !sustained {
            while shared_tx_active_thread_count.load(Ordering::Relaxed) > 0 {
                sleep(Duration::from_millis(100));
            }
        }
        // It's not feasible (would take too much time) to confirm each of the `tx_count / 2`
        // transactions sent by `generate_txs()` so instead send and confirm a single transaction
        // to validate the network is still functional.
        send_barrier_transaction(
            &mut barrier_client,
            &mut blockhash,
            &barrier_source_keypair,
            &barrier_dest_id,
        );

        i += 1;
        if should_switch_directions(num_lamports_per_account, i) {
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

    let balance = client.poll_get_balance(&id.pubkey()).unwrap_or(0);
    metrics_submit_lamport_balance(balance);

    compute_and_report_stats(
        &maxes,
        sample_period,
        &start.elapsed(),
        total_tx_sent_count.load(Ordering::Relaxed),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
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
}
