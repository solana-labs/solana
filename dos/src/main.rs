//! DoS tool
//!
//! Sends requests to cluster in a loop to measure
//! the effect of handling these requests on the performance of the cluster.
//!
//! * `mode` argument defines interface to use (e.g. rpc, tvu, tpu)
//! * `data-type` argument specifies the type of the request.
//! Some request types might be used only with particular `mode` value.
//! For example, `get-account-info` is valid only with `mode=rpc`.
//!
//! Most options are provided for `data-type = transaction`.
//! These options allow to compose transaction which fails at
//! a particular stage of the processing pipeline.
//!
//! To limit the number of possible options and simplify the usage of the tool,
//! The following configurations are suggested:
//! Let `COMMON="--mode tpu --data-type transaction --unique-transactions"`
//! 1. Without blockhash or payer:
//! 1.1 With invalid signatures
//! ```bash
//! solana-dos $COMMON --num-signatures 8
//! ```
//! 1.2 With valid signatures
//! ```bash
//! solana-dos $COMMON --valid-signatures --num-signatures 8
//! ```
//! 2. With blockhash and payer:
//! 2.1 Single-instruction transaction
//! ```bash
//! solana-dos $COMMON --valid-blockhash --transaction-type transfer --num-instructions 1
//! ```
//! 2.2 Multi-instruction transaction
//! ```bash
//! solana-dos $COMMON --valid-blockhash --transaction-type transfer --num-instructions 8
//! ```
//! 2.3 Account-creation transaction
//! ```bash
//! solana-dos $COMMON --valid-blockhash --transaction-type account-creation
//! ```
//!
#![allow(clippy::arithmetic_side_effects)]
use {
    crossbeam_channel::{select, tick, unbounded, Receiver, Sender},
    itertools::Itertools,
    log::*,
    rand::{thread_rng, Rng},
    solana_bench_tps::{bench::generate_and_fund_keypairs, bench_tps_client::BenchTpsClient},
    solana_client::{connection_cache::ConnectionCache, tpu_connection::TpuConnection},
    solana_core::repair::serve_repair::{RepairProtocol, RepairRequestHeader, ServeRepair},
    solana_dos::cli::*,
    solana_gossip::{
        contact_info::Protocol,
        gossip_service::{discover, get_multi_client},
        legacy_contact_info::LegacyContactInfo as ContactInfo,
    },
    solana_measure::measure::Measure,
    solana_rpc_client::rpc_client::RpcClient,
    solana_sdk::{
        hash::Hash,
        instruction::CompiledInstruction,
        message::Message,
        pubkey::Pubkey,
        signature::{Keypair, Signature, Signer},
        stake,
        system_instruction::{self, SystemInstruction},
        system_program,
        timing::timestamp,
        transaction::Transaction,
    },
    solana_streamer::socket::SocketAddrSpace,
    solana_tpu_client::tpu_client::DEFAULT_TPU_CONNECTION_POOL_SIZE,
    std::{
        net::{SocketAddr, UdpSocket},
        process::exit,
        sync::Arc,
        thread,
        time::{Duration, Instant},
    },
};

const PROGRESS_TIMEOUT_S: u64 = 120;
const SAMPLE_PERIOD_MS: u64 = 10_000;
fn compute_rate_per_second(count: usize) -> usize {
    (count * 1000) / (SAMPLE_PERIOD_MS as usize)
}

/// Provide functionality to generate several types of transactions:
///
/// 1. Without blockhash
/// 1.1 With valid signatures (number of signatures is configurable)
/// 1.2 With invalid signatures (number of signatures is configurable)
///
/// 2. With blockhash (but still deliberately invalid):
/// 2.1 Transfer from 1 payer to multiple destinations (many instructions per transaction)
/// 2.2 Create an account
///
#[derive(Clone)]
struct TransactionGenerator {
    blockhash: Hash,
    last_generated: Instant,
    transaction_params: TransactionParams,
}

impl TransactionGenerator {
    fn new(transaction_params: TransactionParams) -> Self {
        TransactionGenerator {
            blockhash: Hash::default(),
            last_generated: Instant::now()
                .checked_sub(Duration::from_secs(100))
                .unwrap(), //to force generation when generate is called
            transaction_params,
        }
    }

    /// Generate transaction
    ///
    /// `payer` - the account responsible for paying the cost of executing transaction, used as
    /// a source for transfer instructions and as funding account for create-account instructions.
    /// `destinations` - depending on the transaction type, might be destination accounts receiving transfers,
    /// new accounts, signing accounts. It is `None` only if `valid_signatures==false`.
    /// `client` - structure responsible for providing blockhash.
    ///
    fn generate<T: 'static + BenchTpsClient + Send + Sync>(
        &mut self,
        payer: Option<&Keypair>,
        destinations: Option<Vec<&Keypair>>,
        client: Option<&Arc<T>>,
    ) -> Transaction {
        if self.transaction_params.valid_blockhash {
            let client = client.as_ref().unwrap();
            let destinations = destinations.unwrap();
            let payer = payer.as_ref().unwrap();
            self.generate_with_blockhash(payer, destinations, client)
        } else {
            self.generate_without_blockhash(destinations)
        }
    }

    fn generate_with_blockhash<T: 'static + BenchTpsClient + Send + Sync>(
        &mut self,
        payer: &Keypair,
        destinations: Vec<&Keypair>,
        client: &Arc<T>,
    ) -> Transaction {
        // generate a new blockhash every 1sec
        if self.transaction_params.valid_blockhash
            && self.last_generated.elapsed().as_millis() > 1000
        {
            self.blockhash = client.get_latest_blockhash().unwrap();
            self.last_generated = Instant::now();
        }

        // transaction_type is known to be present because it is required by blockhash option in cli
        let transaction_type = self.transaction_params.transaction_type.as_ref().unwrap();
        match transaction_type {
            TransactionType::Transfer => {
                self.create_multi_transfer_transaction(payer, &destinations)
            }
            TransactionType::AccountCreation => {
                self.create_account_transaction(payer, destinations[0])
            }
        }
    }

    /// Create a transaction which transfers some lamports from payer to several destinations
    fn create_multi_transfer_transaction(&self, payer: &Keypair, to: &[&Keypair]) -> Transaction {
        let to_transfer: u64 = 500_000_000; // specify amount which will cause error
        let to: Vec<(Pubkey, u64)> = to.iter().map(|to| (to.pubkey(), to_transfer)).collect();
        let instructions = system_instruction::transfer_many(&payer.pubkey(), to.as_slice());
        let message = Message::new(&instructions, Some(&payer.pubkey()));
        let mut tx = Transaction::new_unsigned(message);
        tx.sign(&[payer], self.blockhash);
        tx
    }

    /// Create a transaction which opens account
    fn create_account_transaction(&self, payer: &Keypair, to: &Keypair) -> Transaction {
        let program_id = system_program::id(); // some valid program id
        let balance = 500_000_000;
        let space = 1024;
        let instructions = vec![system_instruction::create_account(
            &payer.pubkey(),
            &to.pubkey(),
            balance,
            space,
            &program_id,
        )];

        let message = Message::new(&instructions, Some(&payer.pubkey()));
        let signers: Vec<&Keypair> = vec![payer, to];
        Transaction::new(&signers, message, self.blockhash)
    }

    fn generate_without_blockhash(
        &mut self,
        destinations: Option<Vec<&Keypair>>, // provided for valid signatures
    ) -> Transaction {
        // create an arbitrary valid instruction
        let lamports = 5;
        let transfer_instruction = SystemInstruction::Transfer { lamports };
        let program_ids = vec![system_program::id(), stake::program::id()];
        let instructions = vec![CompiledInstruction::new(
            0,
            &transfer_instruction,
            vec![0, 1],
        )];

        if self.transaction_params.valid_signatures {
            // Since we don't provide a payer, this transaction will end up
            // filtered at legacy.rs sanitize method (banking_stage) with error "a program cannot be payer"
            let destinations = destinations.unwrap();
            Transaction::new_with_compiled_instructions(
                &destinations,
                &[],
                self.blockhash,
                program_ids,
                instructions,
            )
        } else {
            // Since we provided invalid signatures
            // this transaction will end up filtered at legacy.rs (banking_stage) because
            // num_required_signatures == 0
            let mut tx = Transaction::new_with_compiled_instructions(
                &[] as &[&Keypair; 0],
                &[],
                self.blockhash,
                program_ids,
                instructions,
            );
            let num_signatures = self.transaction_params.num_signatures.unwrap();
            tx.signatures = vec![Signature::new_unique(); num_signatures];
            tx
        }
    }
}

// Multithreading-related functions
//
// The most computationally expensive work is signing new transactions.
// Here we generate them in `num_gen_threads` threads.
//
struct TransactionBatchMsg {
    batch: Vec<Vec<u8>>,
    gen_time: u64,
}

/// Creates thread which receives batches of transactions from tx_receiver
/// and sends them to the target.
/// If `iterations` is 0, it works indefenetely.
/// Otherwise, it sends at least `iterations` number of transactions
fn create_sender_thread(
    tx_receiver: Receiver<TransactionBatchMsg>,
    iterations: usize,
    target: &SocketAddr,
    tpu_use_quic: bool,
) -> thread::JoinHandle<()> {
    // ConnectionCache is used instead of client because it gives ~6% higher pps
    let connection_cache = match tpu_use_quic {
        true => ConnectionCache::new_quic(
            "connection_cache_dos_quic",
            DEFAULT_TPU_CONNECTION_POOL_SIZE,
        ),
        false => {
            ConnectionCache::with_udp("connection_cache_dos_udp", DEFAULT_TPU_CONNECTION_POOL_SIZE)
        }
    };
    let connection = connection_cache.get_connection(target);

    let stats_timer_receiver = tick(Duration::from_millis(SAMPLE_PERIOD_MS));
    let progress_timer_receiver = tick(Duration::from_secs(PROGRESS_TIMEOUT_S));

    let mut time_send_ns = 0;
    let mut time_generate_ns = 0;

    // Sender signals to stop Generators by dropping receiver.
    // It happens in 2 cases:
    // * Sender has sent at least `iterations` number of transactions
    // * Sender observes that there is no progress. Since there is no way to use recv_timeout with select,
    // a timer is used.
    thread::Builder::new().name("Sender".to_string()).spawn(move || {
        let mut total_count: usize = 0;
        let mut prev_total_count = 0; // to track progress

        let mut stats_count: usize = 0;
        let mut stats_error_count: usize = 0;

        loop {
            select! {
                recv(tx_receiver) -> msg => {
                    match msg {
                        Ok(tx_batch) => {
                            let len = tx_batch.batch.len();
                            let mut measure_send_txs = Measure::start("measure_send_txs");
                            let res = connection.send_data_batch_async(tx_batch.batch);

                            measure_send_txs.stop();
                            time_send_ns += measure_send_txs.as_ns();
                            time_generate_ns += tx_batch.gen_time;

                            if res.is_err() {
                                stats_error_count += len;
                            }
                            stats_count += len;
                            total_count += len;
                            if iterations != 0 && total_count >= iterations {
                                info!("All transactions has been sent");
                                // dropping receiver to signal generator threads to stop
                                drop(tx_receiver);
                                break;
                            }
                        }
                        _ => panic!("Sender panics"),
                    }
                },
                recv(stats_timer_receiver) -> _ => {
                    info!("tx_receiver queue len: {}", tx_receiver.len());
                    info!("Count: {}, error count: {}, send mean time: {}, generate mean time: {}, rps: {}",
                        stats_count,
                        stats_error_count,
                        time_send_ns.checked_div(stats_count as u64).unwrap_or(0),
                        time_generate_ns.checked_div(stats_count as u64).unwrap_or(0),
                        compute_rate_per_second(stats_count),
                    );
                    stats_count = 0;
                    stats_error_count = 0;
                    time_send_ns = 0;
                    time_generate_ns = 0;
                },
                recv(progress_timer_receiver) -> _ => {
                    if prev_total_count - total_count == 0 {
                        info!("No progress, stop execution");
                        // dropping receiver to signal generator threads to stop
                        drop(tx_receiver);
                        break;
                    }
                    prev_total_count = total_count;
                }
            }
        }
    }).unwrap()
}

fn create_generator_thread<T: 'static + BenchTpsClient + Send + Sync>(
    tx_sender: &Sender<TransactionBatchMsg>,
    send_batch_size: usize,
    transaction_generator: &TransactionGenerator,
    client: Option<Arc<T>>,
    payer: Option<Keypair>,
) -> thread::JoinHandle<()> {
    let tx_sender = tx_sender.clone();

    let mut transaction_generator = transaction_generator.clone();
    let transaction_params: &TransactionParams = &transaction_generator.transaction_params;

    // Generate n=1000 unique keypairs
    // The number of chunks is described by binomial coefficient
    // and hence this choice of n provides large enough number of permutations
    let mut keypairs_flat: Vec<Keypair> = Vec::new();
    // 1000 is arbitrary number. In case of permutation_size > 1,
    // this guaranties large enough set of unique permutations
    let permutation_size = get_permutation_size(
        transaction_params.num_signatures.as_ref(),
        transaction_params.num_instructions.as_ref(),
    );
    let num_keypairs = 1000 * permutation_size;

    let generate_keypairs =
        transaction_params.valid_signatures || transaction_params.valid_blockhash;
    if generate_keypairs {
        keypairs_flat = (0..num_keypairs).map(|_| Keypair::new()).collect();
    }

    thread::Builder::new()
        .name("Generator".to_string())
        .spawn(move || {
            let indexes: Vec<usize> = (0..keypairs_flat.len()).collect();
            let mut it = indexes.iter().permutations(permutation_size);

            loop {
                let mut data = Vec::<Vec<u8>>::with_capacity(send_batch_size);
                let mut measure_generate_txs = Measure::start("measure_generate_txs");
                for _ in 0..send_batch_size {
                    let chunk_keypairs = if generate_keypairs {
                        let mut permutation = it.next();
                        if permutation.is_none() {
                            // if ran out of permutations, regenerate keys
                            keypairs_flat.iter_mut().for_each(|v| *v = Keypair::new());
                            info!("Regenerate keypairs");
                            permutation = it.next();
                        }
                        let permutation = permutation.unwrap();
                        Some(apply_permutation(permutation, &keypairs_flat))
                    } else {
                        None
                    };
                    let tx = transaction_generator.generate(
                        payer.as_ref(),
                        chunk_keypairs,
                        client.as_ref(),
                    );
                    data.push(bincode::serialize(&tx).unwrap());
                }
                measure_generate_txs.stop();

                let result = tx_sender.send(TransactionBatchMsg {
                    batch: data,
                    gen_time: measure_generate_txs.as_ns(),
                });
                if result.is_err() {
                    // means that receiver has been dropped by sender thread
                    info!("Exit generator thread");
                    break;
                }
            }
        })
        .unwrap()
}

fn get_target(
    nodes: &[ContactInfo],
    mode: Mode,
    entrypoint_addr: SocketAddr,
    tpu_use_quic: bool,
) -> Option<(Pubkey, SocketAddr)> {
    let protocol = if tpu_use_quic {
        Protocol::QUIC
    } else {
        Protocol::UDP
    };
    let mut target = None;
    if nodes.is_empty() {
        // skip-gossip case
        target = Some((solana_sdk::pubkey::new_rand(), entrypoint_addr));
    } else {
        info!("************ NODE ***********");
        for node in nodes {
            info!("{:?}", node);
        }
        info!("ADDR = {}", entrypoint_addr);

        for node in nodes {
            if node.gossip().ok() == Some(entrypoint_addr) {
                info!("{:?}", node.gossip());
                target = match mode {
                    Mode::Gossip => Some((*node.pubkey(), node.gossip().unwrap())),
                    Mode::Tvu => Some((*node.pubkey(), node.tvu(Protocol::UDP).unwrap())),
                    Mode::Tpu => Some((*node.pubkey(), node.tpu(protocol).unwrap())),
                    Mode::TpuForwards => {
                        Some((*node.pubkey(), node.tpu_forwards(protocol).unwrap()))
                    }
                    Mode::Repair => todo!("repair socket is not gossiped anymore!"),
                    Mode::ServeRepair => {
                        Some((*node.pubkey(), node.serve_repair(Protocol::UDP).unwrap()))
                    }
                    Mode::Rpc => None,
                };
                break;
            }
        }
    }
    target
}

fn get_rpc_client(
    nodes: &[ContactInfo],
    entrypoint_addr: SocketAddr,
) -> Result<RpcClient, &'static str> {
    if nodes.is_empty() {
        // skip-gossip case
        return Ok(RpcClient::new_socket(entrypoint_addr));
    }

    // find target node
    for node in nodes {
        if node.gossip().ok() == Some(entrypoint_addr) {
            info!("{:?}", node.gossip());
            return Ok(RpcClient::new_socket(node.rpc().unwrap()));
        }
    }
    Err("Node with entrypoint_addr was not found")
}

fn run_dos_rpc_mode_helper<F: Fn() -> bool>(iterations: usize, rpc_client_call: F) {
    let mut last_log = Instant::now();
    let mut total_count: usize = 0;
    let mut count = 0;
    let mut error_count = 0;
    loop {
        if !rpc_client_call() {
            error_count += 1;
        }
        count += 1;
        total_count += 1;
        if last_log.elapsed().as_millis() > SAMPLE_PERIOD_MS as u128 {
            info!(
                "count: {}, errors: {}, rps: {}",
                count,
                error_count,
                compute_rate_per_second(count)
            );
            last_log = Instant::now();
            count = 0;
        }
        if iterations != 0 && total_count >= iterations {
            break;
        }
    }
}

fn run_dos_rpc_mode(
    rpc_client: RpcClient,
    iterations: usize,
    data_type: DataType,
    data_input: &Pubkey,
) {
    match data_type {
        DataType::GetAccountInfo => {
            run_dos_rpc_mode_helper(iterations, || -> bool {
                rpc_client.get_account(data_input).is_ok()
            });
        }
        DataType::GetProgramAccounts => {
            run_dos_rpc_mode_helper(iterations, || -> bool {
                rpc_client.get_program_accounts(data_input).is_ok()
            });
        }
        _ => {
            panic!("unsupported data type");
        }
    }
}

/// Apply given permutation to the vector of items
fn apply_permutation<'a, T>(permutation: Vec<&usize>, items: &'a [T]) -> Vec<&'a T> {
    let mut res = Vec::with_capacity(permutation.len());
    for i in permutation {
        res.push(&items[*i]);
    }
    res
}

fn create_payers<T: 'static + BenchTpsClient + Send + Sync>(
    valid_blockhash: bool,
    size: usize,
    client: Option<&Arc<T>>,
) -> Vec<Option<Keypair>> {
    // Assume that if we use valid blockhash, we also have a payer
    if valid_blockhash {
        // each payer is used to fund transaction
        // transactions are built to be invalid so the the amount here is arbitrary
        let funding_key = Keypair::new();
        let funding_key = Arc::new(funding_key);
        let res = generate_and_fund_keypairs(
            client.unwrap().clone(),
            &funding_key,
            size,
            1_000_000,
            false,
        )
        .unwrap_or_else(|e| {
            eprintln!("Error could not fund keys: {e:?}");
            exit(1);
        });
        res.into_iter().map(Some).collect()
    } else {
        std::iter::repeat_with(|| None).take(size).collect()
    }
}

fn get_permutation_size(num_signatures: Option<&usize>, num_instructions: Option<&usize>) -> usize {
    if let Some(num_signatures) = num_signatures {
        *num_signatures
    } else if let Some(num_instructions) = num_instructions {
        *num_instructions
    } else {
        1 // for the case AccountCreation
    }
}

fn run_dos_transactions<T: 'static + BenchTpsClient + Send + Sync>(
    target: SocketAddr,
    iterations: usize,
    client: Option<Arc<T>>,
    transaction_params: TransactionParams,
    tpu_use_quic: bool,
    num_gen_threads: usize,
    send_batch_size: usize,
) {
    // Number of payers is the number of generating threads
    // Later, we will create a new payer for each thread since Keypair is not clonable
    let payers: Vec<Option<Keypair>> = create_payers(
        transaction_params.valid_blockhash,
        num_gen_threads,
        client.as_ref(),
    );

    let transaction_generator = TransactionGenerator::new(transaction_params);
    let (tx_sender, tx_receiver) = unbounded();

    let sender_thread = create_sender_thread(tx_receiver, iterations, &target, tpu_use_quic);
    let tx_generator_threads: Vec<_> = payers
        .into_iter()
        .map(|payer| {
            create_generator_thread(
                &tx_sender,
                send_batch_size,
                &transaction_generator,
                client.clone(),
                payer,
            )
        })
        .collect();
    if let Err(err) = sender_thread.join() {
        println!("join() failed with: {err:?}");
    }
    for t_generator in tx_generator_threads {
        if let Err(err) = t_generator.join() {
            println!("join() failed with: {err:?}");
        }
    }
}

fn run_dos<T: 'static + BenchTpsClient + Send + Sync>(
    nodes: &[ContactInfo],
    iterations: usize,
    client: Option<Arc<T>>,
    params: DosClientParameters,
) {
    let target = get_target(
        nodes,
        params.mode,
        params.entrypoint_addr,
        params.tpu_use_quic,
    );
    if params.mode == Mode::Rpc {
        // creating rpc_client because get_account, get_program_accounts are not implemented for BenchTpsClient
        let rpc_client =
            get_rpc_client(nodes, params.entrypoint_addr).expect("Failed to get rpc client");
        // existence of data_input is checked at cli level
        run_dos_rpc_mode(
            rpc_client,
            iterations,
            params.data_type,
            &params.data_input.unwrap(),
        );
    } else if params.data_type == DataType::Transaction
        && params.transaction_params.unique_transactions
    {
        let (_, target_addr) = target.expect("should have target");
        info!("Targeting {}", target_addr);
        run_dos_transactions(
            target_addr,
            iterations,
            client,
            params.transaction_params,
            params.tpu_use_quic,
            params.num_gen_threads,
            params.send_batch_size,
        );
    } else {
        let (target_id, target_addr) = target.expect("should have target");
        info!("Targeting {}", target_addr);
        let mut data = match params.data_type {
            DataType::RepairHighest => {
                let slot = 100;
                let keypair = Keypair::new();
                let header = RepairRequestHeader::new(keypair.pubkey(), target_id, timestamp(), 0);
                let req = RepairProtocol::WindowIndex {
                    header,
                    slot,
                    shred_index: 0,
                };
                ServeRepair::repair_proto_to_bytes(&req, &keypair).unwrap()
            }
            DataType::RepairShred => {
                let slot = 100;
                let keypair = Keypair::new();
                let header = RepairRequestHeader::new(keypair.pubkey(), target_id, timestamp(), 0);
                let req = RepairProtocol::HighestWindowIndex {
                    header,
                    slot,
                    shred_index: 0,
                };
                ServeRepair::repair_proto_to_bytes(&req, &keypair).unwrap()
            }
            DataType::RepairOrphan => {
                let slot = 100;
                let keypair = Keypair::new();
                let header = RepairRequestHeader::new(keypair.pubkey(), target_id, timestamp(), 0);
                let req = RepairProtocol::Orphan { header, slot };
                ServeRepair::repair_proto_to_bytes(&req, &keypair).unwrap()
            }
            DataType::Random => {
                vec![0; params.data_size]
            }
            DataType::Transaction => {
                let tp = params.transaction_params;
                info!("{:?}", tp);

                let valid_blockhash = tp.valid_blockhash;
                let payers: Vec<Option<Keypair>> =
                    create_payers(valid_blockhash, 1, client.as_ref());
                let payer = payers[0].as_ref();

                let permutation_size =
                    get_permutation_size(tp.num_signatures.as_ref(), tp.num_instructions.as_ref());
                let keypairs: Vec<Keypair> =
                    (0..permutation_size).map(|_| Keypair::new()).collect();
                let keypairs_chunk: Option<Vec<&Keypair>> =
                    if tp.valid_signatures || tp.valid_blockhash {
                        Some(keypairs.iter().collect())
                    } else {
                        None
                    };

                let mut transaction_generator = TransactionGenerator::new(tp);
                let tx = transaction_generator.generate(payer, keypairs_chunk, client.as_ref());
                info!("{:?}", tx);
                bincode::serialize(&tx).unwrap()
            }
            _ => panic!("Unsupported data_type detected"),
        };

        let socket = UdpSocket::bind("0.0.0.0:0").unwrap();
        let mut last_log = Instant::now();
        let mut total_count: usize = 0;
        let mut count: usize = 0;
        let mut error_count = 0;
        loop {
            if params.data_type == DataType::Random {
                thread_rng().fill(&mut data[..]);
            }
            let res = socket.send_to(&data, target_addr);
            if res.is_err() {
                error_count += 1;
            }

            count += 1;
            total_count += 1;
            if last_log.elapsed().as_millis() > SAMPLE_PERIOD_MS as u128 {
                info!(
                    "count: {}, errors: {}, rps: {}",
                    count,
                    error_count,
                    compute_rate_per_second(count)
                );
                last_log = Instant::now();
                count = 0;
            }
            if iterations != 0 && total_count >= iterations {
                break;
            }
        }
    }
}

fn main() {
    solana_logger::setup_with_default("solana=info");
    let cmd_params = build_cli_parameters();

    let (nodes, client) = if !cmd_params.skip_gossip {
        info!("Finding cluster entry: {:?}", cmd_params.entrypoint_addr);
        let socket_addr_space = SocketAddrSpace::new(cmd_params.allow_private_addr);
        let (gossip_nodes, validators) = discover(
            None, // keypair
            Some(&cmd_params.entrypoint_addr),
            None,                              // num_nodes
            Duration::from_secs(60),           // timeout
            None,                              // find_nodes_by_pubkey
            Some(&cmd_params.entrypoint_addr), // find_node_by_gossip_addr
            None,                              // my_gossip_addr
            0,                                 // my_shred_version
            socket_addr_space,
        )
        .unwrap_or_else(|err| {
            eprintln!(
                "Failed to discover {} node: {:?}",
                cmd_params.entrypoint_addr, err
            );
            exit(1);
        });

        let connection_cache = match cmd_params.tpu_use_quic {
            true => ConnectionCache::new_quic(
                "connection_cache_dos_quic",
                DEFAULT_TPU_CONNECTION_POOL_SIZE,
            ),
            false => ConnectionCache::with_udp(
                "connection_cache_dos_udp",
                DEFAULT_TPU_CONNECTION_POOL_SIZE,
            ),
        };
        let (client, num_clients) = get_multi_client(
            &validators,
            &SocketAddrSpace::Unspecified,
            Arc::new(connection_cache),
        );
        if validators.len() < num_clients {
            eprintln!(
                "Error: Insufficient nodes discovered.  Expecting {} or more",
                validators.len()
            );
            exit(1);
        }
        (gossip_nodes, Some(Arc::new(client)))
    } else {
        (vec![], None)
    };

    info!("done found {} nodes", nodes.len());

    run_dos(&nodes, 0, client, cmd_params);
}

#[cfg(test)]
pub mod test {
    use {
        super::*,
        solana_client::thin_client::ThinClient,
        solana_core::validator::ValidatorConfig,
        solana_faucet::faucet::run_local_faucet,
        solana_gossip::contact_info::LegacyContactInfo,
        solana_local_cluster::{
            cluster::Cluster,
            local_cluster::{ClusterConfig, LocalCluster},
            validator_configs::make_identical_validator_configs,
        },
        solana_rpc::rpc::JsonRpcConfig,
        solana_sdk::timing::timestamp,
    };

    const TEST_SEND_BATCH_SIZE: usize = 1;

    // thin wrapper for the run_dos function
    // to avoid specifying everywhere generic parameters
    fn run_dos_no_client(nodes: &[ContactInfo], iterations: usize, params: DosClientParameters) {
        run_dos::<ThinClient>(nodes, iterations, None, params);
    }

    #[test]
    fn test_dos() {
        let nodes = [ContactInfo::new_localhost(
            &solana_sdk::pubkey::new_rand(),
            timestamp(),
        )];
        let entrypoint_addr = nodes[0].gossip().unwrap();

        run_dos_no_client(
            &nodes,
            1,
            DosClientParameters {
                entrypoint_addr,
                mode: Mode::Tvu,
                data_size: 10,
                data_type: DataType::Random,
                data_input: None,
                skip_gossip: false,
                allow_private_addr: false,
                num_gen_threads: 1,
                transaction_params: TransactionParams::default(),
                tpu_use_quic: false,
                send_batch_size: TEST_SEND_BATCH_SIZE,
            },
        );

        // TODO: Figure out how to DOS repair. Repair socket is no longer
        // gossiped and cannot be obtained from a node's contact-info.
        #[cfg(not(test))]
        run_dos_no_client(
            &nodes,
            1,
            DosClientParameters {
                entrypoint_addr,
                mode: Mode::Repair,
                data_size: 10,
                data_type: DataType::RepairHighest,
                data_input: None,
                skip_gossip: false,
                allow_private_addr: false,
                num_gen_threads: 1,
                transaction_params: TransactionParams::default(),
                tpu_use_quic: false,
                send_batch_size: TEST_SEND_BATCH_SIZE,
            },
        );

        run_dos_no_client(
            &nodes,
            1,
            DosClientParameters {
                entrypoint_addr,
                mode: Mode::ServeRepair,
                data_size: 10,
                data_type: DataType::RepairShred,
                data_input: None,
                skip_gossip: false,
                allow_private_addr: false,
                num_gen_threads: 1,
                transaction_params: TransactionParams::default(),
                tpu_use_quic: false,
                send_batch_size: TEST_SEND_BATCH_SIZE,
            },
        );

        run_dos_no_client(
            &nodes,
            1,
            DosClientParameters {
                entrypoint_addr,
                mode: Mode::Rpc,
                data_size: 0,
                data_type: DataType::GetAccountInfo,
                data_input: Some(Pubkey::default()),
                skip_gossip: false,
                allow_private_addr: false,
                num_gen_threads: 1,
                transaction_params: TransactionParams::default(),
                tpu_use_quic: false,
                send_batch_size: TEST_SEND_BATCH_SIZE,
            },
        );
    }

    #[test]
    fn test_dos_random() {
        solana_logger::setup();
        let num_nodes = 1;
        let cluster =
            LocalCluster::new_with_equal_stakes(num_nodes, 100, 3, SocketAddrSpace::Unspecified);
        assert_eq!(cluster.validators.len(), num_nodes);

        let nodes = cluster.get_node_pubkeys();
        let node = cluster
            .get_contact_info(&nodes[0])
            .map(LegacyContactInfo::try_from)
            .unwrap()
            .unwrap();
        let nodes_slice = [node];

        // send random transactions to TPU
        // will be discarded on sigverify stage
        run_dos_no_client(
            &nodes_slice,
            10,
            DosClientParameters {
                entrypoint_addr: cluster.entry_point_info.gossip().unwrap(),
                mode: Mode::Tpu,
                data_size: 1024,
                data_type: DataType::Random,
                data_input: None,
                skip_gossip: false,
                allow_private_addr: false,
                num_gen_threads: 1,
                transaction_params: TransactionParams::default(),
                tpu_use_quic: false,
                send_batch_size: TEST_SEND_BATCH_SIZE,
            },
        );
    }

    #[test]
    fn test_dos_without_blockhash() {
        solana_logger::setup();
        let num_nodes = 1;
        let cluster =
            LocalCluster::new_with_equal_stakes(num_nodes, 100, 3, SocketAddrSpace::Unspecified);
        assert_eq!(cluster.validators.len(), num_nodes);

        let nodes = cluster.get_node_pubkeys();
        let node = cluster
            .get_contact_info(&nodes[0])
            .map(LegacyContactInfo::try_from)
            .unwrap()
            .unwrap();
        let nodes_slice = [node];

        let client = Arc::new(ThinClient::new(
            cluster.entry_point_info.rpc().unwrap(),
            cluster
                .entry_point_info
                .tpu(cluster.connection_cache.protocol())
                .unwrap(),
            cluster.connection_cache.clone(),
        ));

        // creates one transaction with 8 valid signatures and sends it 10 times
        run_dos(
            &nodes_slice,
            10,
            Some(client.clone()),
            DosClientParameters {
                entrypoint_addr: cluster.entry_point_info.gossip().unwrap(),
                mode: Mode::Tpu,
                data_size: 0, // irrelevant
                data_type: DataType::Transaction,
                data_input: None,
                skip_gossip: false,
                allow_private_addr: false,
                num_gen_threads: 1,
                transaction_params: TransactionParams {
                    num_signatures: Some(8),
                    valid_blockhash: false,
                    valid_signatures: true,
                    unique_transactions: false,
                    transaction_type: None,
                    num_instructions: None,
                },
                tpu_use_quic: false,
                send_batch_size: TEST_SEND_BATCH_SIZE,
            },
        );

        // creates and sends unique transactions which have invalid signatures
        run_dos(
            &nodes_slice,
            10,
            Some(client.clone()),
            DosClientParameters {
                entrypoint_addr: cluster.entry_point_info.gossip().unwrap(),
                mode: Mode::Tpu,
                data_size: 0, // irrelevant
                data_type: DataType::Transaction,
                data_input: None,
                skip_gossip: false,
                allow_private_addr: false,
                num_gen_threads: 1,
                transaction_params: TransactionParams {
                    num_signatures: Some(8),
                    valid_blockhash: false,
                    valid_signatures: false,
                    unique_transactions: true,
                    transaction_type: None,
                    num_instructions: None,
                },
                tpu_use_quic: false,
                send_batch_size: TEST_SEND_BATCH_SIZE,
            },
        );

        // creates and sends unique transactions which have valid signatures
        run_dos(
            &nodes_slice,
            10,
            Some(client),
            DosClientParameters {
                entrypoint_addr: cluster.entry_point_info.gossip().unwrap(),
                mode: Mode::Tpu,
                data_size: 0, // irrelevant
                data_type: DataType::Transaction,
                data_input: None,
                skip_gossip: false,
                allow_private_addr: false,
                num_gen_threads: 1,
                transaction_params: TransactionParams {
                    num_signatures: Some(8),
                    valid_blockhash: false,
                    valid_signatures: true,
                    unique_transactions: true,
                    transaction_type: None,
                    num_instructions: None,
                },
                tpu_use_quic: false,
                send_batch_size: TEST_SEND_BATCH_SIZE,
            },
        );
    }

    fn run_dos_with_blockhash_and_payer(tpu_use_quic: bool) {
        solana_logger::setup();

        // 1. Create faucet thread
        let faucet_keypair = Keypair::new();
        let faucet_pubkey = faucet_keypair.pubkey();
        let faucet_addr = run_local_faucet(faucet_keypair, None);
        let mut validator_config = ValidatorConfig::default_for_test();
        validator_config.rpc_config = JsonRpcConfig {
            faucet_addr: Some(faucet_addr),
            ..JsonRpcConfig::default_for_test()
        };

        // 2. Create a local cluster which is aware of faucet
        let num_nodes = 1;
        let native_instruction_processors = vec![];
        let cluster = LocalCluster::new(
            &mut ClusterConfig {
                node_stakes: vec![999_990; num_nodes],
                cluster_lamports: 200_000_000,
                validator_configs: make_identical_validator_configs(
                    &ValidatorConfig {
                        rpc_config: JsonRpcConfig {
                            faucet_addr: Some(faucet_addr),
                            ..JsonRpcConfig::default_for_test()
                        },
                        ..ValidatorConfig::default_for_test()
                    },
                    num_nodes,
                ),
                native_instruction_processors,
                ..ClusterConfig::default()
            },
            SocketAddrSpace::Unspecified,
        );
        assert_eq!(cluster.validators.len(), num_nodes);

        // 3. Transfer funds to faucet account
        cluster.transfer(&cluster.funding_keypair, &faucet_pubkey, 100_000_000);

        let nodes = cluster.get_node_pubkeys();
        let node = cluster
            .get_contact_info(&nodes[0])
            .map(LegacyContactInfo::try_from)
            .unwrap()
            .unwrap();
        let nodes_slice = [node];

        let client = Arc::new(ThinClient::new(
            cluster.entry_point_info.rpc().unwrap(),
            cluster
                .entry_point_info
                .tpu(cluster.connection_cache.protocol())
                .unwrap(),
            cluster.connection_cache.clone(),
        ));

        // creates one transaction and sends it 10 times
        // this is done in single thread
        run_dos(
            &nodes_slice,
            10,
            Some(client.clone()),
            DosClientParameters {
                entrypoint_addr: cluster.entry_point_info.gossip().unwrap(),
                mode: Mode::Tpu,
                data_size: 0, // irrelevant if not random
                data_type: DataType::Transaction,
                data_input: None,
                skip_gossip: false,
                allow_private_addr: false,
                num_gen_threads: 1,
                transaction_params: TransactionParams {
                    num_signatures: None,
                    valid_blockhash: true,
                    valid_signatures: true,
                    unique_transactions: false,
                    transaction_type: Some(TransactionType::Transfer),
                    num_instructions: Some(1),
                },
                tpu_use_quic,
                send_batch_size: TEST_SEND_BATCH_SIZE,
            },
        );

        // creates and sends unique transactions of Transfer
        // which tries to send too much lamports from payer to one recipient
        // it uses several threads
        run_dos(
            &nodes_slice,
            10,
            Some(client.clone()),
            DosClientParameters {
                entrypoint_addr: cluster.entry_point_info.gossip().unwrap(),
                mode: Mode::Tpu,
                data_size: 0, // irrelevant if not random
                data_type: DataType::Transaction,
                data_input: None,
                skip_gossip: false,
                allow_private_addr: false,
                num_gen_threads: 1,
                transaction_params: TransactionParams {
                    num_signatures: None,
                    valid_blockhash: true,
                    valid_signatures: true,
                    unique_transactions: true,
                    transaction_type: Some(TransactionType::Transfer),
                    num_instructions: Some(1),
                },
                tpu_use_quic,
                send_batch_size: TEST_SEND_BATCH_SIZE,
            },
        );
        // creates and sends unique transactions of type Transfer
        // which tries to send too much lamports from payer to several recipients
        // it uses several threads
        run_dos(
            &nodes_slice,
            10,
            Some(client.clone()),
            DosClientParameters {
                entrypoint_addr: cluster.entry_point_info.gossip().unwrap(),
                mode: Mode::Tpu,
                data_size: 0, // irrelevant if not random
                data_type: DataType::Transaction,
                data_input: None,
                skip_gossip: false,
                allow_private_addr: false,
                num_gen_threads: 1,
                transaction_params: TransactionParams {
                    num_signatures: None,
                    valid_blockhash: true,
                    valid_signatures: true,
                    unique_transactions: true,
                    transaction_type: Some(TransactionType::Transfer),
                    num_instructions: Some(8),
                },
                tpu_use_quic,
                send_batch_size: TEST_SEND_BATCH_SIZE,
            },
        );
        // creates and sends unique transactions of type CreateAccount
        // which tries to create account with too large balance
        // it uses several threads
        run_dos(
            &nodes_slice,
            10,
            Some(client),
            DosClientParameters {
                entrypoint_addr: cluster.entry_point_info.gossip().unwrap(),
                mode: Mode::Tpu,
                data_size: 0, // irrelevant if not random
                data_type: DataType::Transaction,
                data_input: None,
                skip_gossip: false,
                allow_private_addr: false,
                num_gen_threads: 1,
                transaction_params: TransactionParams {
                    num_signatures: None,
                    valid_blockhash: true,
                    valid_signatures: true,
                    unique_transactions: true,
                    transaction_type: Some(TransactionType::AccountCreation),
                    num_instructions: None,
                },
                tpu_use_quic,
                send_batch_size: TEST_SEND_BATCH_SIZE,
            },
        );
    }

    #[test]
    fn test_dos_with_blockhash_and_payer() {
        run_dos_with_blockhash_and_payer(/*tpu_use_quic*/ false)
    }

    #[test]
    fn test_dos_with_blockhash_and_payer_and_quic() {
        run_dos_with_blockhash_and_payer(/*tpu_use_quic*/ true)
    }
}
