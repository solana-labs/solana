mod bench;
mod cli;

use crate::bench::{do_bench_tps, generate_and_fund_keypairs, Config, NUM_LAMPORTS_PER_ACCOUNT};
use solana::gossip_service::{discover_cluster, get_multi_client};
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
    } = cli_config;

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

    let (keypairs, keypair_balance) = generate_and_fund_keypairs(
        &client,
        Some(drone_addr),
        &id,
        tx_count,
        NUM_LAMPORTS_PER_ACCOUNT,
    );

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
