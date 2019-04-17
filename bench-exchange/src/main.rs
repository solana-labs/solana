pub mod bench;
mod cli;
pub mod order_book;

use crate::bench::{airdrop_lamports, do_bench_exchange, Config};
use log::*;
use solana::cluster_info::FULLNODE_PORT_RANGE;
use solana::gossip_service::discover_nodes;
use solana_client::thin_client::create_client;
use solana_client::thin_client::ThinClient;
use solana_sdk::signature::KeypairUtil;

fn main() {
    solana_logger::setup();

    let matches = cli::build_args().get_matches();
    let cli_config = cli::extract_args(&matches);

    let cli::Config {
        network_addr,
        drone_addr,
        identity,
        threads,
        num_nodes,
        duration,
        trade_delay,
        fund_amount,
        batch_size,
        account_groups,
        ..
    } = cli_config;

    info!("Connecting to the cluster");
    let nodes = discover_nodes(&network_addr, num_nodes).unwrap_or_else(|_| {
        panic!("Failed to discover nodes");
    });
    info!("{} nodes found", nodes.len());
    if nodes.len() < num_nodes {
        panic!("Error: Insufficient nodes discovered");
    }

    let client_ctors: Vec<_> = nodes
        .iter()
        .map(|node| {
            let cluster_entrypoint = node.clone();
            let cluster_addrs = cluster_entrypoint.client_facing_addr();
            move || -> ThinClient { create_client(cluster_addrs, FULLNODE_PORT_RANGE) }
        })
        .collect();

    info!("Funding keypair: {}", identity.pubkey());

    let client = client_ctors[0]();
    let accounts_in_groups = batch_size * account_groups;
    airdrop_lamports(
        &client,
        &drone_addr,
        &identity,
        fund_amount * (accounts_in_groups + 1) as u64 * 2,
    );

    let config = Config {
        identity,
        threads,
        duration,
        trade_delay,
        fund_amount,
        batch_size,
        account_groups,
    };

    do_bench_exchange(client_ctors, config);
}
