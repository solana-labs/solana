mod bench;
mod cli;

use crate::bench::{do_bench_tps, generate_and_fund_keypairs, Config, NUM_LAMPORTS_PER_ACCOUNT};
use solana::cluster_info::FULLNODE_PORT_RANGE;
use solana::contact_info::ContactInfo;
use solana::gossip_service::discover_nodes;
use solana_client::thin_client::create_client;
use std::process::exit;

fn main() {
    solana_logger::setup();
    solana_metrics::set_panic_hook("bench-tps");

    let matches = cli::build_args().get_matches();
    let cli_config = cli::extract_args(&matches);

    let cli::Config {
        network_addr,
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
    let nodes = discover_nodes(&network_addr, num_nodes).unwrap_or_else(|err| {
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
    let clients: Vec<_> = nodes
        .iter()
        .filter_map(|node| {
            let cluster_entrypoint = node.clone();
            let cluster_addrs = cluster_entrypoint.client_facing_addr();
            if ContactInfo::is_valid_address(&cluster_addrs.0)
                && ContactInfo::is_valid_address(&cluster_addrs.1)
            {
                let client = create_client(cluster_addrs, FULLNODE_PORT_RANGE);
                Some(client)
            } else {
                None
            }
        })
        .collect();

    let (keypairs, keypair_balance) = generate_and_fund_keypairs(
        &clients[0],
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

    do_bench_tps(clients, config, keypairs, keypair_balance);
}
