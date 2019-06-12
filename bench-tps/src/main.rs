mod bench;
mod cli;

use crate::bench::{
    do_bench_tps, generate_and_fund_keypairs, generate_keypairs, Config, NUM_LAMPORTS_PER_ACCOUNT,
};
use solana::gossip_service::{discover_cluster, get_multi_client};
use solana_sdk::signature::Keypair;
use std::collections::HashMap;
use std::fs::File;
use std::io::prelude::*;
use std::path::Path;
use std::process::exit;

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
    } = cli_config;

    if write_to_client_file {
        let keypairs = generate_keypairs(&id, tx_count as u64 * 2);
        let mut accounts = HashMap::new();
        keypairs.iter().for_each(|keypair| {
            accounts.insert(
                serde_json::to_string(&keypair.to_bytes().to_vec()).unwrap(),
                NUM_LAMPORTS_PER_ACCOUNT as u64,
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
        (keypairs, last_balance)
    } else {
        generate_and_fund_keypairs(
            &client,
            Some(drone_addr),
            &id,
            tx_count,
            NUM_LAMPORTS_PER_ACCOUNT,
        )
    };

    let config = Config {
        id,
        threads,
        thread_batch_sleep_ms,
        duration,
        tx_count,
        sustained,
    };

    do_bench_tps(vec![client], config, keypairs, keypair_balance);
}
