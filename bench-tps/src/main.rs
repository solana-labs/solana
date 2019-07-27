mod bench;
mod cli;

use crate::bench::{
    do_bench_tps, generate_and_fund_keypairs, generate_keypairs, Config, NUM_LAMPORTS_PER_ACCOUNT,
};
use solana::gossip_service::{discover_cluster, get_multi_client};
use solana_sdk::fee_calculator::FeeCalculator;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Keypair, KeypairUtil};
use std::collections::HashMap;
use std::fs::File;
use std::io::prelude::*;
use std::path::Path;
use std::process::exit;

/// Number of signatures for all transactions in ~1 week at ~100K TPS
pub const NUM_SIGNATURES_FOR_TXS: u64 = 100_000 * 60 * 60 * 24 * 7;

fn main() {
    solana_logger::setup();
    solana_metrics::set_panic_hook("bench-tps");

    let matches = cli::build_args().get_matches();
    let cli_config = cli::extract_args(&matches);

    let cli::Config {
        entrypoint_addr,
        drone_addr,
        id,
        threads,
        num_nodes,
        duration,
        tx_count,
        thread_batch_sleep_ms,
        sustained,
        client_ids_and_stake_file,
        write_to_client_file,
        read_from_client_file,
        target_lamports_per_signature,
    } = cli_config;

    if write_to_client_file {
        let (keypairs, _) = generate_keypairs(&id, tx_count as u64 * 2);
        let num_accounts = keypairs.len() as u64;
        let max_fee = FeeCalculator::new(target_lamports_per_signature).max_lamports_per_signature;
        let num_lamports_per_account = (num_accounts - 1 + NUM_SIGNATURES_FOR_TXS * max_fee)
            / num_accounts
            + NUM_LAMPORTS_PER_ACCOUNT;
        let mut accounts = HashMap::new();
        keypairs.iter().for_each(|keypair| {
            accounts.insert(
                serde_json::to_string(&keypair.to_bytes().to_vec()).unwrap(),
                num_lamports_per_account,
            );
        });

        let serialized = serde_yaml::to_string(&accounts).unwrap();
        let path = Path::new(&client_ids_and_stake_file);
        let mut file = File::create(path).unwrap();
        file.write_all(&serialized.into_bytes()).unwrap();
        return;
    }

    println!("Connecting to the cluster");
    let (nodes, _replicators) =
        discover_cluster(&entrypoint_addr, num_nodes).unwrap_or_else(|err| {
            eprintln!("Failed to discover {} nodes: {:?}", num_nodes, err);
            exit(1);
        });

    let (client, num_clients) = get_multi_client(&nodes);

    if nodes.len() < num_clients {
        eprintln!(
            "Error: Insufficient nodes discovered.  Expecting {} or more",
            num_nodes
        );
        exit(1);
    }

    let (keypairs, keypair_balance) = if read_from_client_file {
        let path = Path::new(&client_ids_and_stake_file);
        let file = File::open(path).unwrap();

        let accounts: HashMap<String, u64> = serde_yaml::from_reader(file).unwrap();
        let mut keypairs = vec![];
        let mut last_balance = 0;

        accounts.into_iter().for_each(|(keypair, balance)| {
            let bytes: Vec<u8> = serde_json::from_str(keypair.as_str()).unwrap();
            keypairs.push(Keypair::from_bytes(&bytes).unwrap());
            last_balance = balance;
        });
        // Sort keypairs so that do_bench_tps() uses the same subset of accounts for each run.
        // This prevents the amount of storage needed for bench-tps accounts from creeping up
        // across multiple runs.
        keypairs.sort_by(|x, y| x.pubkey().to_string().cmp(&y.pubkey().to_string()));
        (keypairs, last_balance)
    } else {
        generate_and_fund_keypairs(
            &client,
            Some(drone_addr),
            &id,
            tx_count,
            NUM_LAMPORTS_PER_ACCOUNT,
            None,
        )
        .unwrap_or_else(|e| {
            eprintln!("Error could not fund keys: {:?}", e);
            exit(1);
        })
    };

    let config = Config {
        id,
        threads,
        thread_batch_sleep_ms,
        duration,
        tx_count,
        sustained,
    };

    do_bench_tps(
        vec![client],
        config,
        keypairs,
        keypair_balance,
        &Pubkey::new_rand(),
        &Pubkey::new_rand(),
    );
}
