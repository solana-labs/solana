use log::*;
use solana_bench_tps::bench::{do_bench_tps, generate_and_fund_keypairs, generate_keypairs};
use solana_bench_tps::cli;
use solana_core::gossip_service::{discover_cluster, get_client, get_multi_client};
use solana_genesis::Base64Account;
use solana_sdk::fee_calculator::FeeCalculator;
use solana_sdk::signature::{Keypair, Signer};
use solana_sdk::system_program;
use std::{collections::HashMap, fs::File, io::prelude::*, path::Path, process::exit, sync::Arc};

/// Number of signatures for all transactions in ~1 week at ~100K TPS
pub const NUM_SIGNATURES_FOR_TXS: u64 = 100_000 * 60 * 60 * 24 * 7;

fn main() {
    solana_logger::setup_with_default("solana=info");
    solana_metrics::set_panic_hook("bench-tps");

    let matches = cli::build_args(solana_clap_utils::version!()).get_matches();
    let cli_config = cli::extract_args(&matches);

    let cli::Config {
        entrypoint_addr,
        faucet_addr,
        id,
        num_nodes,
        tx_count,
        keypair_multiplier,
        client_ids_and_stake_file,
        write_to_client_file,
        read_from_client_file,
        target_lamports_per_signature,
        use_move,
        multi_client,
        num_lamports_per_account,
        ..
    } = &cli_config;

    let keypair_count = *tx_count * keypair_multiplier;
    if *write_to_client_file {
        info!("Generating {} keypairs", keypair_count);
        let (keypairs, _) = generate_keypairs(&id, keypair_count as u64);
        let num_accounts = keypairs.len() as u64;
        let max_fee =
            FeeCalculator::new(*target_lamports_per_signature, 0).max_lamports_per_signature;
        let num_lamports_per_account = (num_accounts - 1 + NUM_SIGNATURES_FOR_TXS * max_fee)
            / num_accounts
            + num_lamports_per_account;
        let mut accounts = HashMap::new();
        keypairs.iter().for_each(|keypair| {
            accounts.insert(
                serde_json::to_string(&keypair.to_bytes().to_vec()).unwrap(),
                Base64Account {
                    balance: num_lamports_per_account,
                    executable: false,
                    owner: system_program::id().to_string(),
                    data: String::new(),
                },
            );
        });

        info!("Writing {}", client_ids_and_stake_file);
        let serialized = serde_yaml::to_string(&accounts).unwrap();
        let path = Path::new(&client_ids_and_stake_file);
        let mut file = File::create(path).unwrap();
        file.write_all(&serialized.into_bytes()).unwrap();
        return;
    }

    info!("Connecting to the cluster");
    let (nodes, _archivers) =
        discover_cluster(&entrypoint_addr, *num_nodes).unwrap_or_else(|err| {
            eprintln!("Failed to discover {} nodes: {:?}", num_nodes, err);
            exit(1);
        });

    let client = if *multi_client {
        let (client, num_clients) = get_multi_client(&nodes);
        if nodes.len() < num_clients {
            eprintln!(
                "Error: Insufficient nodes discovered.  Expecting {} or more",
                num_nodes
            );
            exit(1);
        }
        Arc::new(client)
    } else {
        Arc::new(get_client(&nodes))
    };

    let (keypairs, move_keypairs) = if *read_from_client_file && !use_move {
        let path = Path::new(&client_ids_and_stake_file);
        let file = File::open(path).unwrap();

        info!("Reading {}", client_ids_and_stake_file);
        let accounts: HashMap<String, Base64Account> = serde_yaml::from_reader(file).unwrap();
        let mut keypairs = vec![];
        let mut last_balance = 0;

        accounts
            .into_iter()
            .for_each(|(keypair, primordial_account)| {
                let bytes: Vec<u8> = serde_json::from_str(keypair.as_str()).unwrap();
                keypairs.push(Keypair::from_bytes(&bytes).unwrap());
                last_balance = primordial_account.balance;
            });

        if keypairs.len() < keypair_count {
            eprintln!(
                "Expected {} accounts in {}, only received {} (--tx_count mismatch?)",
                keypair_count,
                client_ids_and_stake_file,
                keypairs.len(),
            );
            exit(1);
        }
        // Sort keypairs so that do_bench_tps() uses the same subset of accounts for each run.
        // This prevents the amount of storage needed for bench-tps accounts from creeping up
        // across multiple runs.
        keypairs.sort_by(|x, y| x.pubkey().to_string().cmp(&y.pubkey().to_string()));
        (keypairs, None)
    } else {
        generate_and_fund_keypairs(
            client.clone(),
            Some(*faucet_addr),
            &id,
            keypair_count,
            *num_lamports_per_account,
            *use_move,
        )
        .unwrap_or_else(|e| {
            eprintln!("Error could not fund keys: {:?}", e);
            exit(1);
        })
    };

    do_bench_tps(client, cli_config, keypairs, move_keypairs);
}
