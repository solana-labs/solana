pub mod bench;
mod cli;
pub mod order_book;

use crate::bench::{airdrop_lamports, do_bench_exchange, Config};
use log::*;
use solana::cluster_info::FULLNODE_PORT_RANGE;
use solana::contact_info::ContactInfo;
use solana::gossip_service::discover_nodes;
use solana_client::thin_client::create_client;
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

    info!("{} nodes found", clients.len());
    if clients.len() < num_nodes {
        panic!("Error: Insufficient nodes discovered");
    }

    info!("Funding keypair: {}", identity.pubkey());

    let accounts_in_groups = batch_size * account_groups;
    airdrop_lamports(
        &clients[0],
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

    do_bench_exchange(clients, config);
}
