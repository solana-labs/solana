mod bench;
mod cli;

use crate::bench::{
    airdrop_lamports, do_bench_tps, fund_keys, generate_keypairs, Config, NUM_LAMPORTS_PER_ACCOUNT,
};
use solana::cluster_info::FULLNODE_PORT_RANGE;
use solana::contact_info::ContactInfo;
use solana::gossip_service::discover_nodes;
use solana_client::thin_client::create_client;
use solana_sdk::client::SyncClient;
use solana_sdk::signature::KeypairUtil;
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

    println!("Creating {} keypairs...", tx_count * 2);
    let keypairs = generate_keypairs(&id, tx_count);

    println!("Get lamports...");

    // Sample the first keypair, see if it has lamports, if so then resume.
    // This logic is to prevent lamport loss on repeated solana-bench-tps executions
    let keypair0_balance = clients[0]
        .get_balance(&keypairs.last().unwrap().pubkey())
        .unwrap_or(0);

    if NUM_LAMPORTS_PER_ACCOUNT > keypair0_balance {
        let extra = NUM_LAMPORTS_PER_ACCOUNT - keypair0_balance;
        let total = extra * (keypairs.len() as u64);
        airdrop_lamports(&clients[0], &drone_addr, &id, total);
        println!("adding more lamports {}", extra);
        fund_keys(&clients[0], &id, &keypairs, extra);
    }

    let config = Config {
        id,
        threads,
        thread_batch_sleep_ms,
        duration,
        tx_count,
        sustained,
    };

    do_bench_tps(clients, config, keypairs, keypair0_balance);
}
