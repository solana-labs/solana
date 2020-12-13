use clap::{crate_description, crate_name, value_t, value_t_or_exit, App, Arg};
use log::*;
use rand::{thread_rng, Rng};
use solana_client::rpc_client::RpcClient;
use solana_core::gossip_service::discover;
use solana_faucet::faucet::{request_airdrop_transaction, FAUCET_PORT};
use solana_measure::measure::Measure;
use solana_sdk::rpc_port::DEFAULT_RPC_PORT;
use solana_sdk::signature::{read_keypair_file, Keypair, Signature, Signer};
use solana_sdk::timing::timestamp;
use solana_sdk::{message::Message, transaction::Transaction};
use solana_sdk::{system_instruction, system_program};
use std::net::SocketAddr;
use std::process::exit;
use std::thread::sleep;
use std::time::Duration;
use std::time::Instant;

pub fn airdrop_lamports(
    client: &RpcClient,
    faucet_addr: &SocketAddr,
    id: &Keypair,
    desired_balance: u64,
) -> bool {
    let starting_balance = client.get_balance(&id.pubkey()).unwrap_or(0);
    info!("starting balance {}", starting_balance);

    if starting_balance < desired_balance {
        let airdrop_amount = desired_balance - starting_balance;
        info!(
            "Airdropping {:?} lamports from {} for {}",
            airdrop_amount,
            faucet_addr,
            id.pubkey(),
        );

        let (blockhash, _fee_calculator) = client.get_recent_blockhash().unwrap();
        match request_airdrop_transaction(&faucet_addr, &id.pubkey(), airdrop_amount, blockhash) {
            Ok(transaction) => {
                let mut tries = 0;
                loop {
                    tries += 1;
                    let signature = client.send_transaction(&transaction).unwrap();
                    let result = client.poll_for_signature(&signature);

                    if result.is_ok() {
                        break;
                    }
                    if tries >= 5 {
                        panic!(
                            "Error requesting airdrop: to addr: {:?} amount: {} {:?}",
                            faucet_addr, airdrop_amount, result
                        )
                    }
                }
            }
            Err(err) => {
                panic!(
                    "Error requesting airdrop: {:?} to addr: {:?} amount: {}",
                    err, faucet_addr, airdrop_amount
                );
            }
        };

        let current_balance = client.get_balance(&id.pubkey()).unwrap_or_else(|e| {
            panic!("airdrop error {}", e);
        });
        info!("current balance {}...", current_balance);

        if current_balance - starting_balance != airdrop_amount {
            info!(
                "Airdrop failed? {} {} {} {}",
                id.pubkey(),
                current_balance,
                starting_balance,
                airdrop_amount,
            );
        }
    }
    true
}

fn run_accounts_bench(
    entrypoint_addr: SocketAddr,
    faucet_addr: SocketAddr,
    keypair: &Keypair,
    iterations: usize,
    maybe_space: Option<u64>,
    batch_size: usize,
    keep_sigs: bool,
    maybe_lamports: Option<u64>,
    num_instructions: usize,
) {
    assert!(num_instructions > 0);
    let client = RpcClient::new_socket(entrypoint_addr);

    info!("Targetting {}", entrypoint_addr);

    let mut last_blockhash = Instant::now();
    let mut last_log = Instant::now();
    let mut count = 0;
    let mut error_count = 0;
    let mut sigs: Vec<(Signature, u64)> = vec![];
    let mut keypairs = vec![];
    let mut recent_blockhash = client.get_recent_blockhash().expect("blockhash");
    let mut tx_sent_count = 0;
    let mut total_account_count = 0;
    let mut success = 0;
    let mut balance = client.get_balance(&keypair.pubkey()).unwrap_or(0);
    let mut last_balance = Instant::now();
    let mut timed_out = 0;

    let default_max_lamports = 1000;
    let min_balance = maybe_lamports.unwrap_or_else(|| {
        let space = maybe_space.unwrap_or(default_max_lamports);
        client
            .get_minimum_balance_for_rent_exemption(space as usize)
            .expect("min balance")
    });
    let mut from_empty = Instant::now();

    info!("Starting balance: {}", balance);

    loop {
        if last_blockhash.elapsed().as_millis() > 10_000 {
            recent_blockhash = client.get_recent_blockhash().expect("blockhash");
            last_blockhash = Instant::now();
        }

        let space = maybe_space.unwrap_or_else(|| thread_rng().gen_range(0, 1000));

        let (instructions, mut new_keypairs): (Vec<_>, Vec<_>) = (0..num_instructions)
            .into_iter()
            .map(|_| {
                let new_keypair = Keypair::new();

                (
                    system_instruction::create_account(
                        &keypair.pubkey(),
                        &new_keypair.pubkey(),
                        min_balance,
                        space,
                        &system_program::id(),
                    ),
                    new_keypair,
                )
            })
            .unzip();

        let signers: Vec<&Keypair> = new_keypairs
            .iter()
            .chain(std::iter::once(keypair))
            .collect();
        let message = Message::new(&instructions, Some(&keypair.pubkey()));
        let fee = recent_blockhash.1.calculate_fee(&message);
        let lamports = min_balance + fee;

        if balance < lamports || last_balance.elapsed().as_millis() > 2000 {
            if let Ok(b) = client.get_balance(&keypair.pubkey()) {
                balance = b;
            }
            last_balance = Instant::now();
            if balance < lamports {
                info!(
                    "Balance {} is less than needed: {}, doing aidrop...",
                    balance, lamports
                );
                if !airdrop_lamports(&client, &faucet_addr, keypair, lamports * 100_000) {
                    warn!("failed airdrop, exiting");
                    return;
                }
            }
        }

        let tx = Transaction::new(&signers, message, recent_blockhash.0);

        if keep_sigs {
            keypairs.extend(new_keypairs.drain(..));
        }

        if sigs.len() >= batch_size {
            let time = from_empty.elapsed();
            let mut start = Measure::start("sig_status");
            let statuses: Vec<_> = sigs
                .chunks(200)
                .map(|sig_chunk| {
                    let only_sigs: Vec<_> = sig_chunk.iter().map(|s| s.0).collect();
                    client
                        .get_signature_statuses(&only_sigs)
                        .expect("status fail")
                        .value
                })
                .flatten()
                .collect();
            let mut i = 0;
            let mut cleared = 0;
            let start_len = sigs.len();
            let now = timestamp();
            sigs.retain(|(_sig, sent_ts)| {
                let mut retain = true;
                if let Some(e) = &statuses[i] {
                    debug!("error: {:?}", e);
                    if e.status.is_ok() {
                        success += 1;
                    } else {
                        error_count += 1;
                    }
                    cleared += 1;
                    retain = false;
                } else if now - sent_ts > 30_000 {
                    retain = false;
                    timed_out += 1;
                }
                i += 1;
                retain
            });
            start.stop();
            info!(
                "sigs len: {:?} {} took: {}ms cleared: {}/{}",
                sigs.len(),
                start,
                time.as_millis(),
                cleared,
                start_len,
            );
        } else {
            if sigs.is_empty() {
                from_empty = Instant::now();
            }
            balance -= lamports;
            match client.send_transaction(&tx) {
                Ok(sig) => {
                    sigs.push((sig, timestamp()));
                    tx_sent_count += 1;
                    total_account_count += num_instructions;
                }
                Err(e) => {
                    info!("error: {:#?}", e);
                }
            }
        }

        count += 1;
        if last_log.elapsed().as_secs() > 2 {
            info!(
                "total_accounts: {} tx_sent_count: {} loop_count: {} success: {} errors: {} timed_out: {} balance: {}",
                total_account_count, tx_sent_count, count, success, error_count, timed_out, balance
            );
            last_log = Instant::now();
        }
        if iterations != 0 && count >= iterations {
            break;
        }
        if sigs.len() >= batch_size {
            sleep(Duration::from_millis(500));
        }
    }
}

fn main() {
    solana_logger::setup_with_default("solana=info");
    let matches = App::new(crate_name!())
        .about(crate_description!())
        .version(solana_version::version!())
        .arg(
            Arg::with_name("entrypoint")
                .long("entrypoint")
                .takes_value(true)
                .value_name("HOST:PORT")
                .help("RPC entrypoint address. Usually <ip>:8899"),
        )
        .arg(
            Arg::with_name("faucet_addr")
                .long("faucet")
                .takes_value(true)
                .value_name("HOST:PORT")
                .help("Faucet entrypoint address. Usually <ip>:9900"),
        )
        .arg(
            Arg::with_name("space")
                .long("space")
                .takes_value(true)
                .value_name("BYTES")
                .help("Size of accounts to create"),
        )
        .arg(
            Arg::with_name("lamports")
                .long("lamports")
                .takes_value(true)
                .value_name("LAMPORTS")
                .help("How many lamports to fund each account"),
        )
        .arg(
            Arg::with_name("identity")
                .long("identity")
                .takes_value(true)
                .value_name("FILE")
                .help("keypair file"),
        )
        .arg(
            Arg::with_name("batch_size")
                .long("batch_size")
                .takes_value(true)
                .value_name("BYTES")
                .help("Size of accounts to create"),
        )
        .arg(
            Arg::with_name("num_instructions")
                .long("num_instructions")
                .takes_value(true)
                .value_name("NUM")
                .help("Number of accounts to create on each transaction"),
        )
        .arg(
            Arg::with_name("check_gossip")
                .long("check-gossip")
                .help("Just use entrypoint address directly"),
        )
        .get_matches();

    let skip_gossip = !matches.is_present("check_gossip");

    let port = if skip_gossip { DEFAULT_RPC_PORT } else { 8001 };
    let mut entrypoint_addr = SocketAddr::from(([127, 0, 0, 1], port));
    if let Some(addr) = matches.value_of("entrypoint") {
        entrypoint_addr = solana_net_utils::parse_host_port(addr).unwrap_or_else(|e| {
            eprintln!("failed to parse entrypoint address: {}", e);
            exit(1)
        });
    }
    let mut faucet_addr = SocketAddr::from(([127, 0, 0, 1], FAUCET_PORT));
    if let Some(addr) = matches.value_of("faucet_addr") {
        faucet_addr = solana_net_utils::parse_host_port(addr).unwrap_or_else(|e| {
            eprintln!("failed to parse entrypoint address: {}", e);
            exit(1)
        });
    }

    let keep_sigs = matches.is_present("keep_sigs");

    let space = value_t!(matches, "space", u64).ok();
    let lamports = value_t!(matches, "lamports", u64).ok();
    let batch_size = value_t!(matches, "batch_size", usize).unwrap_or(4);
    let iterations = value_t!(matches, "iterations", usize).unwrap_or(10);
    let num_instructions = value_t!(matches, "num_instructions", usize).unwrap_or(1);
    if num_instructions == 0 || num_instructions > 500 {
        eprintln!("bad num_instructions: {}", num_instructions);
        exit(1);
    }

    let keypair =
        read_keypair_file(&value_t_or_exit!(matches, "identity", String)).expect("bad keypair");

    let mut nodes = vec![];
    if !skip_gossip {
        info!("Finding cluster entry: {:?}", entrypoint_addr);
        let (gossip_nodes, _validators) = discover(
            None,
            Some(&entrypoint_addr),
            None,
            Some(60),
            None,
            Some(&entrypoint_addr),
            None,
            0,
        )
        .unwrap_or_else(|err| {
            eprintln!("Failed to discover {} node: {:?}", entrypoint_addr, err);
            exit(1);
        });
        nodes = gossip_nodes;
    }

    info!("done found {} nodes", nodes.len());

    run_accounts_bench(
        entrypoint_addr,
        faucet_addr,
        &keypair,
        iterations,
        space,
        batch_size,
        keep_sigs,
        lamports,
        num_instructions,
    );
}

#[cfg(test)]
pub mod test {
    use super::*;
    use solana_core::validator::ValidatorConfig;
    use solana_local_cluster::local_cluster::{ClusterConfig, LocalCluster};
    use solana_sdk::poh_config::PohConfig;

    #[test]
    fn test_accounts_cluster_bench() {
        solana_logger::setup();
        let validator_config = ValidatorConfig::default();
        let num_nodes = 1;
        let mut config = ClusterConfig {
            cluster_lamports: 10_000_000,
            poh_config: PohConfig::new_sleep(Duration::from_millis(50)),
            node_stakes: vec![100; num_nodes],
            validator_configs: vec![validator_config; num_nodes],
            ..ClusterConfig::default()
        };

        let faucet_addr = SocketAddr::from(([127, 0, 0, 1], 9900));
        let cluster = LocalCluster::new(&mut config);
        let iterations = 10;
        let maybe_space = None;
        let keep_sigs = false;
        let batch_size = 4;
        let maybe_lamports = None;
        let num_instructions = 2;
        run_accounts_bench(
            cluster.entry_point_info.rpc,
            faucet_addr,
            &cluster.funding_keypair,
            iterations,
            maybe_space,
            batch_size,
            keep_sigs,
            maybe_lamports,
            num_instructions,
        );
    }
}
