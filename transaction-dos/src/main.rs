#![allow(clippy::arithmetic_side_effects)]

use {
    clap::{crate_description, crate_name, value_t, values_t_or_exit, App, Arg},
    log::*,
    rand::{thread_rng, Rng},
    rayon::prelude::*,
    solana_clap_utils::input_parsers::pubkey_of,
    solana_cli::{
        cli::{process_command, CliCommand, CliConfig},
        program::ProgramCliCommand,
    },
    solana_client::transaction_executor::TransactionExecutor,
    solana_faucet::faucet::{request_airdrop_transaction, FAUCET_PORT},
    solana_gossip::gossip_service::discover,
    solana_rpc_client::rpc_client::RpcClient,
    solana_sdk::{
        commitment_config::CommitmentConfig,
        instruction::{AccountMeta, Instruction},
        message::Message,
        packet::PACKET_DATA_SIZE,
        pubkey::Pubkey,
        rpc_port::DEFAULT_RPC_PORT,
        signature::{read_keypair_file, Keypair, Signer},
        system_instruction,
        transaction::Transaction,
    },
    solana_streamer::socket::SocketAddrSpace,
    std::{
        net::{Ipv4Addr, SocketAddr},
        process::exit,
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc,
        },
        thread::sleep,
        time::{Duration, Instant},
    },
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

        let blockhash = client.get_latest_blockhash().unwrap();
        match request_airdrop_transaction(faucet_addr, &id.pubkey(), airdrop_amount, blockhash) {
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
                            "Error requesting airdrop: to addr: {faucet_addr:?} amount: {airdrop_amount} {result:?}"
                        )
                    }
                }
            }
            Err(err) => {
                panic!(
                    "Error requesting airdrop: {err:?} to addr: {faucet_addr:?} amount: {airdrop_amount}"
                );
            }
        };

        let current_balance = client.get_balance(&id.pubkey()).unwrap_or_else(|e| {
            panic!("airdrop error {e}");
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

fn make_create_message(
    keypair: &Keypair,
    base_keypair: &Keypair,
    balance: u64,
    space: u64,
    program_id: Pubkey,
) -> Message {
    let instructions = vec![system_instruction::create_account(
        &keypair.pubkey(),
        &base_keypair.pubkey(),
        balance,
        space,
        &program_id,
    )];

    Message::new(&instructions, Some(&keypair.pubkey()))
}

fn make_dos_message(
    keypair: &Keypair,
    num_instructions: usize,
    program_id: Pubkey,
    num_program_iterations: u8,
    account_metas: &[AccountMeta],
) -> Message {
    let instructions: Vec<_> = (0..num_instructions)
        .map(|_| {
            let data = [num_program_iterations, thread_rng().gen_range(0..255)];
            Instruction::new_with_bytes(program_id, &data, account_metas.to_vec())
        })
        .collect();

    Message::new(&instructions, Some(&keypair.pubkey()))
}

/// creates large transactions that all touch the same set of accounts,
/// so they can't be parallelized
///
#[allow(clippy::too_many_arguments)]
fn run_transactions_dos(
    entrypoint_addr: SocketAddr,
    faucet_addr: SocketAddr,
    payer_keypairs: &[&Keypair],
    iterations: usize,
    maybe_space: Option<u64>,
    batch_size: usize,
    maybe_lamports: Option<u64>,
    num_instructions: usize,
    num_program_iterations: usize,
    program_id: Pubkey,
    program_options: Option<(Keypair, String)>,
    account_keypairs: &[&Keypair],
    maybe_account_groups: Option<usize>,
    just_calculate_fees: bool,
    batch_sleep_ms: u64,
) {
    assert!(num_instructions > 0);
    let client = Arc::new(RpcClient::new_socket_with_commitment(
        entrypoint_addr,
        CommitmentConfig::confirmed(),
    ));

    info!("Targeting {}", entrypoint_addr);

    let space = maybe_space.unwrap_or(1000);

    let min_balance = maybe_lamports.unwrap_or_else(|| {
        client
            .get_minimum_balance_for_rent_exemption(space as usize)
            .expect("min balance")
    });
    assert!(min_balance > 0);

    let account_groups = maybe_account_groups.unwrap_or(1);

    assert!(account_keypairs.len() % account_groups == 0);

    let account_group_size = account_keypairs.len() / account_groups;

    let program_account = client.get_account(&program_id);

    let mut blockhash = client.get_latest_blockhash().expect("blockhash");
    let mut message = Message::new_with_blockhash(
        &[
            Instruction::new_with_bytes(
                Pubkey::new_unique(),
                &[],
                vec![AccountMeta::new(Pubkey::new_unique(), true)],
            ),
            Instruction::new_with_bytes(
                Pubkey::new_unique(),
                &[],
                vec![AccountMeta::new(Pubkey::new_unique(), true)],
            ),
        ],
        None,
        &blockhash,
    );

    let mut latest_blockhash = Instant::now();
    let mut last_log = Instant::now();
    let mut count = 0;

    if just_calculate_fees {
        let fee = client
            .get_fee_for_message(&message)
            .expect("get_fee_for_message");

        let account_space_fees = min_balance * account_keypairs.len() as u64;
        let program_fees = if program_account.is_ok() {
            0
        } else {
            // todo, dynamic real size
            client.get_minimum_balance_for_rent_exemption(2400).unwrap()
        };
        let transaction_fees =
            account_keypairs.len() as u64 * fee + iterations as u64 * batch_size as u64 * fee;
        info!(
            "Accounts fees: {} program_account fees: {} transaction fees: {} total: {}",
            account_space_fees,
            program_fees,
            transaction_fees,
            account_space_fees + program_fees + transaction_fees,
        );
        return;
    }

    if program_account.is_err() {
        let mut config = CliConfig::default();
        let (program_keypair, program_location) = program_options
            .expect("If the program doesn't exist, need to provide program keypair to deploy");
        info!(
            "processing deploy: {:?} key: {}",
            program_account,
            program_keypair.pubkey()
        );
        config.signers = vec![payer_keypairs[0], &program_keypair];
        config.command = CliCommand::Program(ProgramCliCommand::Deploy {
            program_location: Some(program_location),
            program_signer_index: Some(1),
            program_pubkey: None,
            buffer_signer_index: None,
            buffer_pubkey: None,
            allow_excessive_balance: true,
            upgrade_authority_signer_index: 0,
            is_final: true,
            max_len: None,
            skip_fee_check: true, // skip_fee_check
        });

        process_command(&config).expect("deploy didn't pass");
    } else {
        info!("Found program account. Skipping deploy..");
        assert!(program_account.unwrap().executable);
    }

    let mut tx_sent_count = 0;
    let mut total_dos_messages_sent = 0;
    let mut balances: Vec<_> = payer_keypairs
        .iter()
        .map(|keypair| client.get_balance(&keypair.pubkey()).unwrap_or(0))
        .collect();
    let mut last_balance = Instant::now();

    info!("Starting balance(s): {:?}", balances);

    let executor = TransactionExecutor::new(entrypoint_addr);

    let mut accounts_created = false;
    let tested_size = Arc::new(AtomicBool::new(false));

    let account_metas: Vec<_> = account_keypairs
        .iter()
        .map(|kp| AccountMeta::new(kp.pubkey(), false))
        .collect();

    loop {
        if latest_blockhash.elapsed().as_secs() > 10 {
            blockhash = client.get_latest_blockhash().expect("blockhash");
            message.recent_blockhash = blockhash;
            latest_blockhash = Instant::now();
        }

        let fee = client
            .get_fee_for_message(&message)
            .expect("get_fee_for_message");
        let lamports = min_balance + fee;

        for (i, balance) in balances.iter_mut().enumerate() {
            if *balance < lamports || last_balance.elapsed().as_secs() > 2 {
                if let Ok(b) = client.get_balance(&payer_keypairs[i].pubkey()) {
                    *balance = b;
                }
                last_balance = Instant::now();
                if *balance < lamports * 2 {
                    info!(
                        "Balance {} is less than needed: {}, doing aidrop...",
                        balance, lamports
                    );
                    if !airdrop_lamports(
                        &client,
                        &faucet_addr,
                        payer_keypairs[i],
                        lamports * 100_000,
                    ) {
                        warn!("failed airdrop, exiting");
                        return;
                    }
                }
            }
        }

        if !accounts_created {
            let mut accounts_to_create = vec![];
            for kp in account_keypairs {
                if let Ok(account) = client.get_account(&kp.pubkey()) {
                    if account.data.len() as u64 != space {
                        info!(
                            "account {} doesn't have space specified. Has {} requested: {}",
                            kp.pubkey(),
                            account.data.len(),
                            space,
                        );
                    }
                } else {
                    accounts_to_create.push(kp);
                }
            }

            if !accounts_to_create.is_empty() {
                info!("creating accounts {}", accounts_to_create.len());
                let txs: Vec<_> = accounts_to_create
                    .par_iter()
                    .enumerate()
                    .map(|(i, keypair)| {
                        let message = make_create_message(
                            payer_keypairs[i % payer_keypairs.len()],
                            keypair,
                            min_balance,
                            space,
                            program_id,
                        );
                        let signers: Vec<&Keypair> =
                            vec![payer_keypairs[i % payer_keypairs.len()], keypair];
                        Transaction::new(&signers, message, blockhash)
                    })
                    .collect();
                let mut new_ids = executor.push_transactions(txs);
                warn!("sent account creation {}", new_ids.len());
                let start = Instant::now();
                loop {
                    let cleared = executor.drain_cleared();
                    new_ids.retain(|x| !cleared.contains(x));
                    if new_ids.is_empty() {
                        break;
                    }
                    if start.elapsed().as_secs() > 60 {
                        info!("Some creation failed");
                        break;
                    }
                    sleep(Duration::from_millis(500));
                }
                for kp in account_keypairs {
                    let account = client.get_account(&kp.pubkey()).unwrap();
                    info!("{} => {:?}", kp.pubkey(), account);
                    assert!(account.data.len() as u64 == space);
                }
            } else {
                info!("All accounts created.");
            }
            accounts_created = true;
        } else {
            // Create dos transactions
            info!("creating new batch of size: {}", batch_size);
            let chunk_size = batch_size / payer_keypairs.len();
            for (i, keypair) in payer_keypairs.iter().enumerate() {
                let txs: Vec<_> = (0..chunk_size)
                    .into_par_iter()
                    .map(|x| {
                        let message = make_dos_message(
                            keypair,
                            num_instructions,
                            program_id,
                            num_program_iterations as u8,
                            &account_metas[(x % account_groups) * account_group_size
                                ..(x % account_groups) * account_group_size + account_group_size],
                        );
                        let signers: Vec<&Keypair> = vec![keypair];
                        let tx = Transaction::new(&signers, message, blockhash);
                        if !tested_size.load(Ordering::Relaxed) {
                            let ser_size = bincode::serialized_size(&tx).unwrap();
                            assert!(ser_size < PACKET_DATA_SIZE as u64, "{}", ser_size);
                            tested_size.store(true, Ordering::Relaxed);
                        }
                        tx
                    })
                    .collect();
                balances[i] = balances[i].saturating_sub(fee * txs.len() as u64);
                info!("txs: {}", txs.len());
                let new_ids = executor.push_transactions(txs);
                info!("ids: {}", new_ids.len());
                tx_sent_count += new_ids.len();
                total_dos_messages_sent += num_instructions * new_ids.len();
            }
            let _ = executor.drain_cleared();
        }

        count += 1;
        if last_log.elapsed().as_secs() > 3 {
            info!(
                "total_dos_messages_sent: {} tx_sent_count: {} loop_count: {} balance(s): {:?}",
                total_dos_messages_sent, tx_sent_count, count, balances
            );
            last_log = Instant::now();
        }
        if iterations != 0 && count >= iterations {
            break;
        }
        if executor.num_outstanding() >= batch_size {
            sleep(Duration::from_millis(batch_sleep_ms));
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
            Arg::with_name("payer")
                .long("payer")
                .takes_value(true)
                .multiple(true)
                .value_name("FILE")
                .help("One or more payer keypairs to fund account creation."),
        )
        .arg(
            Arg::with_name("account")
                .long("account")
                .takes_value(true)
                .multiple(true)
                .value_name("FILE")
                .help("One or more keypairs to create accounts owned by the program and which the program will write to."),
        )
        .arg(
            Arg::with_name("account_groups")
            .long("account_groups")
            .takes_value(true)
            .value_name("NUM")
            .help("Number of groups of accounts to split the accounts into")
        )
        .arg(
            Arg::with_name("batch_size")
                .long("batch-size")
                .takes_value(true)
                .value_name("NUM")
                .help("Number of transactions to send per batch"),
        )
        .arg(
            Arg::with_name("num_instructions")
                .long("num-instructions")
                .takes_value(true)
                .value_name("NUM")
                .help("Number of accounts to create on each transaction"),
        )
        .arg(
            Arg::with_name("num_program_iterations")
                .long("num-program-iterations")
                .takes_value(true)
                .value_name("NUM")
                .help("Number of iterations in the smart contract"),
        )
        .arg(
            Arg::with_name("iterations")
                .long("iterations")
                .takes_value(true)
                .value_name("NUM")
                .help("Number of iterations to make"),
        )
        .arg(
            Arg::with_name("batch_sleep_ms")
                .long("batch-sleep-ms")
                .takes_value(true)
                .value_name("NUM")
                .help("Sleep for this long the num outstanding transctions is greater than the batch size."),
        )
        .arg(
            Arg::with_name("check_gossip")
                .long("check-gossip")
                .help("Just use entrypoint address directly"),
        )
        .arg(
            Arg::with_name("just_calculate_fees")
                .long("just-calculate-fees")
                .help("Just print the necessary fees and exit"),
        )
        .arg(
            Arg::with_name("program_id")
                .long("program-id")
                .takes_value(true)
                .required(true)
                .help("program_id address to initialize account"),
        )
        .get_matches();

    let skip_gossip = !matches.is_present("check_gossip");
    let just_calculate_fees = matches.is_present("just_calculate_fees");

    let port = if skip_gossip { DEFAULT_RPC_PORT } else { 8001 };
    let mut entrypoint_addr = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    if let Some(addr) = matches.value_of("entrypoint") {
        entrypoint_addr = solana_net_utils::parse_host_port(addr).unwrap_or_else(|e| {
            eprintln!("failed to parse entrypoint address: {e}");
            exit(1)
        });
    }
    let mut faucet_addr = SocketAddr::from((Ipv4Addr::LOCALHOST, FAUCET_PORT));
    if let Some(addr) = matches.value_of("faucet_addr") {
        faucet_addr = solana_net_utils::parse_host_port(addr).unwrap_or_else(|e| {
            eprintln!("failed to parse entrypoint address: {e}");
            exit(1)
        });
    }

    let space = value_t!(matches, "space", u64).ok();
    let lamports = value_t!(matches, "lamports", u64).ok();
    let batch_size = value_t!(matches, "batch_size", usize).unwrap_or(4);
    let iterations = value_t!(matches, "iterations", usize).unwrap_or(10);
    let num_program_iterations = value_t!(matches, "num_program_iterations", usize).unwrap_or(10);
    let num_instructions = value_t!(matches, "num_instructions", usize).unwrap_or(1);
    if num_instructions == 0 || num_instructions > 500 {
        eprintln!("bad num_instructions: {num_instructions}");
        exit(1);
    }
    let batch_sleep_ms = value_t!(matches, "batch_sleep_ms", u64).unwrap_or(500);

    let program_id = pubkey_of(&matches, "program_id").unwrap();

    let payer_keypairs: Vec<_> = values_t_or_exit!(matches, "payer", String)
        .iter()
        .map(|keypair_string| {
            read_keypair_file(keypair_string)
                .unwrap_or_else(|_| panic!("bad keypair {keypair_string:?}"))
        })
        .collect();

    let account_keypairs: Vec<_> = values_t_or_exit!(matches, "account", String)
        .iter()
        .map(|keypair_string| {
            read_keypair_file(keypair_string)
                .unwrap_or_else(|_| panic!("bad keypair {keypair_string:?}"))
        })
        .collect();

    let account_groups = value_t!(matches, "account_groups", usize).ok();
    let payer_keypair_refs: Vec<&Keypair> = payer_keypairs.iter().collect();
    let account_keypair_refs: Vec<&Keypair> = account_keypairs.iter().collect();

    let rpc_addr = if !skip_gossip {
        info!("Finding cluster entry: {:?}", entrypoint_addr);
        let (gossip_nodes, _validators) = discover(
            None, // keypair
            Some(&entrypoint_addr),
            None,                    // num_nodes
            Duration::from_secs(60), // timeout
            None,                    // find_node_by_pubkey
            Some(&entrypoint_addr),  // find_node_by_gossip_addr
            None,                    // my_gossip_addr
            0,                       // my_shred_version
            SocketAddrSpace::Unspecified,
        )
        .unwrap_or_else(|err| {
            eprintln!("Failed to discover {entrypoint_addr} node: {err:?}");
            exit(1);
        });

        info!("done found {} nodes", gossip_nodes.len());
        gossip_nodes[0].rpc().unwrap()
    } else {
        info!("Using {:?} as the RPC address", entrypoint_addr);
        entrypoint_addr
    };

    run_transactions_dos(
        rpc_addr,
        faucet_addr,
        &payer_keypair_refs,
        iterations,
        space,
        batch_size,
        lamports,
        num_instructions,
        num_program_iterations,
        program_id,
        None,
        &account_keypair_refs,
        account_groups,
        just_calculate_fees,
        batch_sleep_ms,
    );
}

#[cfg(test)]
pub mod test {
    use {
        super::*,
        solana_core::validator::ValidatorConfig,
        solana_local_cluster::{
            local_cluster::{ClusterConfig, LocalCluster},
            validator_configs::make_identical_validator_configs,
        },
        solana_measure::measure::Measure,
        solana_sdk::poh_config::PohConfig,
    };

    #[test]
    fn test_tx_size() {
        solana_logger::setup();
        let keypair = Keypair::new();
        let num_instructions = 20;
        let program_id = Pubkey::new_unique();
        let num_accounts = 17;

        let account_metas: Vec<_> = (0..num_accounts)
            .map(|_| AccountMeta::new(Pubkey::new_unique(), false))
            .collect();
        let num_program_iterations = 10;
        let message = make_dos_message(
            &keypair,
            num_instructions,
            program_id,
            num_program_iterations,
            &account_metas,
        );
        let signers: Vec<&Keypair> = vec![&keypair];
        let blockhash = solana_sdk::hash::Hash::default();
        let tx = Transaction::new(&signers, message, blockhash);
        let size = bincode::serialized_size(&tx).unwrap();
        info!("size:{}", size);
        assert!(size < PACKET_DATA_SIZE as u64);
    }

    #[test]
    #[ignore]
    fn test_transaction_dos() {
        solana_logger::setup();

        let validator_config = ValidatorConfig::default_for_test();
        let num_nodes = 1;
        let mut config = ClusterConfig {
            cluster_lamports: 10_000_000,
            poh_config: PohConfig::new_sleep(Duration::from_millis(50)),
            node_stakes: vec![100; num_nodes],
            validator_configs: make_identical_validator_configs(&validator_config, num_nodes),
            ..ClusterConfig::default()
        };

        let faucet_addr = SocketAddr::from((Ipv4Addr::LOCALHOST, 9900));
        let cluster = LocalCluster::new(&mut config, SocketAddrSpace::Unspecified);

        let program_keypair = Keypair::new();

        let iterations = 1000;
        let maybe_space = Some(10_000_000);
        let batch_size = 1;
        let maybe_lamports = Some(10);
        let maybe_account_groups = Some(1);
        // 85 inst, 142 iterations, 5 accounts
        // 20 inst, 30 * 20 iterations, 1 account
        //
        // 100 inst, 7 * 20 iterations, 1 account
        let num_instructions = 70;
        let num_program_iterations = 10;
        let num_accounts = 7;
        let account_keypairs: Vec<_> = (0..num_accounts).map(|_| Keypair::new()).collect();
        let account_keypair_refs: Vec<_> = account_keypairs.iter().collect();
        let mut start = Measure::start("total accounts run");
        run_transactions_dos(
            cluster.entry_point_info.rpc().unwrap(),
            faucet_addr,
            &[&cluster.funding_keypair],
            iterations,
            maybe_space,
            batch_size,
            maybe_lamports,
            num_instructions,
            num_program_iterations,
            program_keypair.pubkey(),
            Some((
                program_keypair,
                format!(
                    "{}{}",
                    env!("CARGO_MANIFEST_DIR"),
                    "/../programs/sbf/c/out/tuner.so"
                ),
            )),
            &account_keypair_refs,
            maybe_account_groups,
            false,
            100,
        );
        start.stop();
        info!("{}", start);
    }
}
