pub mod bench;
mod cli;
pub mod order_book;

#[cfg(test)]
#[macro_use]
extern crate solana_exchange_program;

use crate::bench::{airdrop_lamports, do_bench_exchange, Config};
use log::*;
use solana::gossip_service::{discover_cluster, get_multi_client};
use solana_sdk::signature::KeypairUtil;

fn main() {
    solana_logger::setup();
    solana_metrics::set_panic_hook("bench-exchange");

    let matches = cli::build_args().get_matches();
    let cli_config = cli::extract_args(&matches);

    let cli::Config {
        entrypoint_addr,
        drone_addr,
        identity,
        threads,
        num_nodes,
        duration,
        transfer_delay,
        fund_amount,
        batch_size,
        chunk_size,
        account_groups,
        ..
    } = cli_config;

    info!("Connecting to the cluster");
    let (nodes, _replicators) =
        discover_cluster(&entrypoint_addr, num_nodes).unwrap_or_else(|_| {
            panic!("Failed to discover nodes");
        });

    let (client, num_clients) = get_multi_client(&nodes);

    info!("{} nodes found", num_clients);
    if num_clients < num_nodes {
        panic!("Error: Insufficient nodes discovered");
    }

    info!("Funding keypair: {}", identity.pubkey());

    let accounts_in_groups = batch_size * account_groups;
    const NUM_SIGNERS: u64 = 2;
    airdrop_lamports(
        &client,
        &drone_addr,
        &identity,
        fund_amount * (accounts_in_groups + 1) as u64 * NUM_SIGNERS,
    );

    let config = Config {
        identity,
        threads,
        duration,
        transfer_delay,
        fund_amount,
        batch_size,
        chunk_size,
        account_groups,
    };

    do_bench_exchange(vec![client], config);
}
