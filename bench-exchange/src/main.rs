pub mod bench;
mod cli;
pub mod order_book;

use crate::bench::{airdrop_lamports, do_bench_exchange, get_clients, Config};
use log::*;
use solana::gossip_service::discover_nodes;
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

    let clients = get_clients(nodes);

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
