#![allow(clippy::integer_arithmetic)]
use clap::{crate_description, crate_name, value_t, value_t_or_exit, App, Arg};
use log::*;
use rand::{thread_rng, Rng};
use rayon::prelude::*;
use solana_account_decoder::parse_token::spl_token_v2_0_pubkey;
use solana_clap_utils::input_parsers::pubkey_of;
use solana_client::rpc_client::RpcClient;
use solana_core::gossip_service::discover;
use solana_faucet::faucet::{request_airdrop_transaction, FAUCET_PORT};
use solana_measure::measure::Measure;
use solana_runtime::inline_spl_token_v2_0;
use solana_sdk::{
    commitment_config::CommitmentConfig,
    message::Message,
    pubkey::Pubkey,
    rpc_port::DEFAULT_RPC_PORT,
    signature::{read_keypair_file, Keypair, Signature, Signer},
    system_instruction, system_program,
    timing::timestamp,
    transaction::Transaction,
};
use solana_transaction_status::parse_token::spl_token_v2_0_instruction;
use spl_token_v2_0::solana_program::pubkey::Pubkey as SplPubkey;
use std::{
    net::SocketAddr,
    process::exit,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, RwLock,
    },
    thread::{sleep, Builder, JoinHandle},
    time::{Duration, Instant},
};

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
                    let result = client.send_and_confirm_transaction(&transaction);

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

// signature, timestamp, id
type PendingQueue = Vec<(Signature, u64, u64)>;

struct TransactionExecutor {
    sig_clear_t: JoinHandle<()>,
    sigs: Arc<RwLock<PendingQueue>>,
    cleared: Arc<RwLock<Vec<u64>>>,
    exit: Arc<AtomicBool>,
    counter: AtomicU64,
    client: RpcClient,
}

impl TransactionExecutor {
    fn new(entrypoint_addr: SocketAddr) -> Self {
        let sigs = Arc::new(RwLock::new(Vec::new()));
        let cleared = Arc::new(RwLock::new(Vec::new()));
        let exit = Arc::new(AtomicBool::new(false));
        let sig_clear_t = Self::start_sig_clear_thread(&exit, &sigs, &cleared, entrypoint_addr);
        let client =
            RpcClient::new_socket_with_commitment(entrypoint_addr, CommitmentConfig::confirmed());
        Self {
            sigs,
            cleared,
            sig_clear_t,
            exit,
            counter: AtomicU64::new(0),
            client,
        }
    }

    fn num_outstanding(&self) -> usize {
        self.sigs.read().unwrap().len()
    }

    fn push_transactions(&self, txs: Vec<Transaction>) -> Vec<u64> {
        let mut ids = vec![];
        let new_sigs = txs.into_iter().filter_map(|tx| {
            let id = self.counter.fetch_add(1, Ordering::Relaxed);
            ids.push(id);
            match self.client.send_transaction(&tx) {
                Ok(sig) => {
                    return Some((sig, timestamp(), id));
                }
                Err(e) => {
                    info!("error: {:#?}", e);
                }
            }
            None
        });
        let mut sigs_w = self.sigs.write().unwrap();
        sigs_w.extend(new_sigs);
        ids
    }

    fn drain_cleared(&self) -> Vec<u64> {
        std::mem::take(&mut *self.cleared.write().unwrap())
    }

    fn close(self) {
        self.exit.store(true, Ordering::Relaxed);
        self.sig_clear_t.join().unwrap();
    }

    fn start_sig_clear_thread(
        exit: &Arc<AtomicBool>,
        sigs: &Arc<RwLock<PendingQueue>>,
        cleared: &Arc<RwLock<Vec<u64>>>,
        entrypoint_addr: SocketAddr,
    ) -> JoinHandle<()> {
        let sigs = sigs.clone();
        let exit = exit.clone();
        let cleared = cleared.clone();
        Builder::new()
            .name("sig_clear".to_string())
            .spawn(move || {
                let client = RpcClient::new_socket_with_commitment(
                    entrypoint_addr,
                    CommitmentConfig::confirmed(),
                );
                let mut success = 0;
                let mut error_count = 0;
                let mut timed_out = 0;
                let mut last_log = Instant::now();
                while !exit.load(Ordering::Relaxed) {
                    let sigs_len = sigs.read().unwrap().len();
                    if sigs_len > 0 {
                        let mut sigs_w = sigs.write().unwrap();
                        let mut start = Measure::start("sig_status");
                        let statuses: Vec<_> = sigs_w
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
                        let mut num_cleared = 0;
                        let start_len = sigs_w.len();
                        let now = timestamp();
                        let mut new_ids = vec![];
                        let mut i = 0;
                        let mut j = 0;
                        while i != sigs_w.len() {
                            let mut retain = true;
                            let sent_ts = sigs_w[i].1;
                            if let Some(e) = &statuses[j] {
                                debug!("error: {:?}", e);
                                if e.status.is_ok() {
                                    success += 1;
                                } else {
                                    error_count += 1;
                                }
                                num_cleared += 1;
                                retain = false;
                            } else if now - sent_ts > 30_000 {
                                retain = false;
                                timed_out += 1;
                            }
                            if !retain {
                                new_ids.push(sigs_w.remove(i).2);
                            } else {
                                i += 1;
                            }
                            j += 1;
                        }
                        let final_sigs_len = sigs_w.len();
                        drop(sigs_w);
                        cleared.write().unwrap().extend(new_ids);
                        start.stop();
                        debug!(
                            "sigs len: {:?} success: {} took: {}ms cleared: {}/{}",
                            final_sigs_len,
                            success,
                            start.as_ms(),
                            num_cleared,
                            start_len,
                        );
                        if last_log.elapsed().as_millis() > 5000 {
                            info!(
                                "success: {} error: {} timed_out: {}",
                                success, error_count, timed_out,
                            );
                            last_log = Instant::now();
                        }
                    }
                    sleep(Duration::from_millis(200));
                }
            })
            .unwrap()
    }
}

fn make_message(
    keypair: &Keypair,
    num_instructions: usize,
    balance: u64,
    maybe_space: Option<u64>,
    mint: Option<Pubkey>,
) -> (Message, Vec<Keypair>) {
    let space = maybe_space.unwrap_or_else(|| thread_rng().gen_range(0, 1000));

    let (instructions, new_keypairs): (Vec<_>, Vec<_>) = (0..num_instructions)
        .into_iter()
        .map(|_| {
            let new_keypair = Keypair::new();

            let program_id = if mint.is_some() {
                inline_spl_token_v2_0::id()
            } else {
                system_program::id()
            };
            let mut instructions = vec![system_instruction::create_account(
                &keypair.pubkey(),
                &new_keypair.pubkey(),
                balance,
                space,
                &program_id,
            )];
            if let Some(mint_address) = mint {
                instructions.push(spl_token_v2_0_instruction(
                    spl_token_v2_0::instruction::initialize_account(
                        &spl_token_v2_0::id(),
                        &spl_token_v2_0_pubkey(&new_keypair.pubkey()),
                        &spl_token_v2_0_pubkey(&mint_address),
                        &SplPubkey::new_unique(),
                    )
                    .unwrap(),
                ));
            }

            (instructions, new_keypair)
        })
        .unzip();
    let instructions: Vec<_> = instructions.into_iter().flatten().collect();

    (
        Message::new(&instructions, Some(&keypair.pubkey())),
        new_keypairs,
    )
}

fn run_accounts_bench(
    entrypoint_addr: SocketAddr,
    faucet_addr: SocketAddr,
    keypair: &Keypair,
    iterations: usize,
    maybe_space: Option<u64>,
    batch_size: usize,
    maybe_lamports: Option<u64>,
    num_instructions: usize,
    mint: Option<Pubkey>,
) {
    assert!(num_instructions > 0);
    let client =
        RpcClient::new_socket_with_commitment(entrypoint_addr, CommitmentConfig::confirmed());

    info!("Targetting {}", entrypoint_addr);

    let mut last_blockhash = Instant::now();
    let mut last_log = Instant::now();
    let mut count = 0;
    let mut recent_blockhash = client.get_recent_blockhash().expect("blockhash");
    let mut tx_sent_count = 0;
    let mut total_account_count = 0;
    let mut balance = client.get_balance(&keypair.pubkey()).unwrap_or(0);
    let mut last_balance = Instant::now();

    let default_max_lamports = 1000;
    let min_balance = maybe_lamports.unwrap_or_else(|| {
        let space = maybe_space.unwrap_or(default_max_lamports);
        client
            .get_minimum_balance_for_rent_exemption(space as usize)
            .expect("min balance")
    });

    info!("Starting balance: {}", balance);

    let executor = TransactionExecutor::new(entrypoint_addr);

    loop {
        if last_blockhash.elapsed().as_millis() > 10_000 {
            recent_blockhash = client.get_recent_blockhash().expect("blockhash");
            last_blockhash = Instant::now();
        }

        let (message, _keypairs) =
            make_message(keypair, num_instructions, min_balance, maybe_space, mint);
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

        let sigs_len = executor.num_outstanding();
        if sigs_len < batch_size {
            let num_to_create = batch_size - sigs_len;
            info!("creating {} new", num_to_create);
            let (txs, _new_keypairs): (Vec<_>, Vec<_>) = (0..num_to_create)
                .into_par_iter()
                .map(|_| {
                    let (message, new_keypairs) =
                        make_message(keypair, num_instructions, min_balance, maybe_space, mint);
                    let signers: Vec<&Keypair> = new_keypairs
                        .iter()
                        .chain(std::iter::once(keypair))
                        .collect();
                    (
                        Transaction::new(&signers, message, recent_blockhash.0),
                        new_keypairs,
                    )
                })
                .unzip();
            balance = balance.saturating_sub(lamports * txs.len() as u64);
            info!("txs: {}", txs.len());
            let new_ids = executor.push_transactions(txs);
            info!("ids: {}", new_ids.len());
            tx_sent_count += new_ids.len();
            total_account_count += num_instructions * new_ids.len();
        } else {
            let _ = executor.drain_cleared();
        }

        count += 1;
        if last_log.elapsed().as_millis() > 3000 {
            info!(
                "total_accounts: {} tx_sent_count: {} loop_count: {} balance: {}",
                total_account_count, tx_sent_count, count, balance
            );
            last_log = Instant::now();
        }
        if iterations != 0 && count >= iterations {
            break;
        }
        if executor.num_outstanding() >= batch_size {
            sleep(Duration::from_millis(500));
        }
    }
    executor.close();
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
            Arg::with_name("iterations")
                .long("iterations")
                .takes_value(true)
                .value_name("NUM")
                .help("Number of iterations to make"),
        )
        .arg(
            Arg::with_name("check_gossip")
                .long("check-gossip")
                .help("Just use entrypoint address directly"),
        )
        .arg(
            Arg::with_name("mint")
                .long("mint")
                .takes_value(true)
                .help("Mint address to initialize account"),
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

    let space = value_t!(matches, "space", u64).ok();
    let lamports = value_t!(matches, "lamports", u64).ok();
    let batch_size = value_t!(matches, "batch_size", usize).unwrap_or(4);
    let iterations = value_t!(matches, "iterations", usize).unwrap_or(10);
    let num_instructions = value_t!(matches, "num_instructions", usize).unwrap_or(1);
    if num_instructions == 0 || num_instructions > 500 {
        eprintln!("bad num_instructions: {}", num_instructions);
        exit(1);
    }

    let mint = pubkey_of(&matches, "mint");

    let keypair =
        read_keypair_file(&value_t_or_exit!(matches, "identity", String)).expect("bad keypair");

    let rpc_addr = if !skip_gossip {
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

        info!("done found {} nodes", gossip_nodes.len());
        gossip_nodes[0].rpc
    } else {
        info!("Using {:?} as the RPC address", entrypoint_addr);
        entrypoint_addr
    };

    run_accounts_bench(
        rpc_addr,
        faucet_addr,
        &keypair,
        iterations,
        space,
        batch_size,
        lamports,
        num_instructions,
        mint,
    );
}

#[cfg(test)]
pub mod test {
    use super::*;
    use solana_core::validator::ValidatorConfig;
    use solana_local_cluster::{
        local_cluster::{ClusterConfig, LocalCluster},
        validator_configs::make_identical_validator_configs,
    };
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
            validator_configs: make_identical_validator_configs(&validator_config, num_nodes),
            ..ClusterConfig::default()
        };

        let faucet_addr = SocketAddr::from(([127, 0, 0, 1], 9900));
        let cluster = LocalCluster::new(&mut config);
        let iterations = 10;
        let maybe_space = None;
        let batch_size = 100;
        let maybe_lamports = None;
        let num_instructions = 2;
        let mut start = Measure::start("total accounts run");
        run_accounts_bench(
            cluster.entry_point_info.rpc,
            faucet_addr,
            &cluster.funding_keypair,
            iterations,
            maybe_space,
            batch_size,
            maybe_lamports,
            num_instructions,
            None,
        );
        start.stop();
        info!("{}", start);
    }
}
