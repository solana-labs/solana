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
//! Example 1: send random transactions to TPU
//! ```bash
//! solana-dos --entrypoint 127.0.0.1:8001 --mode tpu --data-type random
//! ```
//!
//! Example 2: send unique transactions with valid recent blockhash to TPU
//! ```bash
//! solana-dos --entrypoint 127.0.0.1:8001 --mode tpu --data-type random
//! solana-dos --entrypoint 127.0.0.1:8001 --mode tpu \
//!     --data-type transaction --generate-unique-transactions
//!     --payer config/bootstrap-validator/identity.json \
//!     --generate-valid-blockhash
//! ```
//!
#![allow(clippy::integer_arithmetic)]

use {
    crossbeam_channel::{select, tick, unbounded, Receiver, Sender},
    itertools::Itertools,
    log::*,
    rand::{thread_rng, Rng},
    solana_bench_tps::bench::generate_and_fund_keypairs,
    solana_client::rpc_client::RpcClient,
    solana_client::transaction_executor::TransactionExecutor,
    solana_core::serve_repair::RepairProtocol,
    solana_dos::cli::*,
    solana_gossip::{contact_info::ContactInfo, gossip_service::discover},
    solana_sdk::{
        client::Client,
        hash::Hash,
        instruction::{AccountMeta, CompiledInstruction, Instruction},
        message::Message,
        pubkey::Pubkey,
        signature::{read_keypair_file, Keypair, Signature, Signer},
        stake, system_instruction,
        system_instruction::SystemInstruction,
        system_program, system_transaction,
        transaction::Transaction,
    },
    solana_streamer::socket::SocketAddrSpace,
    std::{
        net::{SocketAddr, UdpSocket},
        process::exit,
        str::FromStr,
        sync::Arc,
        thread,
        time::{Duration, Instant},
    },
};

static REPORT_EACH_MILLIS: u128 = 10_000;
fn compute_tps(count: usize) -> usize {
    (count * 1000) / (REPORT_EACH_MILLIS as usize)
}

fn get_repair_contact(nodes: &[ContactInfo]) -> ContactInfo {
    let source = thread_rng().gen_range(0, nodes.len());
    let mut contact = nodes[source].clone();
    contact.id = solana_sdk::pubkey::new_rand();
    contact
}

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
            last_generated: (Instant::now() - Duration::from_secs(100)),
            transaction_params,
        }
    }

    fn generate(
        &mut self,
        payer: Option<&Keypair>,
        kpvals: Option<Vec<&Keypair>>, // provided for valid signatures
        rpc_client: &Arc<RpcClient>,
    ) -> Transaction {
        // generate a new blockhash every 1sec
        if self.transaction_params.valid_blockhash
            && self.last_generated.elapsed().as_millis() > 1000
        {
            self.blockhash = rpc_client.as_ref().get_latest_blockhash().unwrap();
            self.last_generated = Instant::now();
        }

        // in order to evaluate the performance implications of the different transactions
        // we create here transactions which are filtered out on different stages of processing pipeline

        // create an arbitrary valid instruction
        let lamports = 5;
        let transfer_instruction = SystemInstruction::Transfer { lamports };
        let program_ids = vec![system_program::id(), stake::program::id()];

        let payer = Some(kpvals.as_ref().unwrap()[0]);
        // transaction with payer, in this case signatures are valid and num_signatures is irrelevant
        // random payer will cause error "attempt to debit an account but found no record of a prior credit"
        // if payer is correct, it will trigger error with not enough signatures
        if let Some(payer) = payer {
            // Option 1: 1->1 transfer
            //let to = Keypair::new(); //kpvals.as_ref().unwrap()[1];
            //system_transaction::transfer(payer, &to.pubkey(), 500_000_000, self.blockhash)

            // Option 2: 1->many
            //let kpvals = kpvals.unwrap();
            //let t: Vec<(Pubkey, u64)> = kpvals.iter().map(|to| (to.pubkey(), 1) ).collect();
            //let instructions = system_instruction::transfer_many(&payer.pubkey(), t.as_slice());
            //let message = Message::new(&instructions, Some(&payer.pubkey()));
            //let mut tx = Transaction::new_unsigned(message);
            //tx.sign(&[payer], self.blockhash);
            //tx

            // Option 3: create account
            let balance = 10;
            let space = 1024;
            let keypair = Keypair::new(); // kpvals.as_ref().unwrap()[1];
            let instructions = vec![system_instruction::create_account(
                &payer.pubkey(),
                &keypair.pubkey(),
                balance,
                space,
                &program_ids[0],
            )];

            let message = Message::new(&instructions, Some(&payer.pubkey()));
            let signers: Vec<&Keypair> = vec![payer, &keypair];
            Transaction::new(&signers, message, self.blockhash)

            /*let instruction = Instruction::new_with_bincode(
                program_ids[0],
                &transfer_instruction,
                vec![
                    AccountMeta::new(program_ids[0], false),
                    AccountMeta::new(program_ids[1], false),
                ],
            );
            Transaction::new_signed_with_payer(
                &[instruction],
                Some(&payer.pubkey()),
                &[payer],
                self.blockhash,
            )*/
        } else if self.transaction_params.valid_signatures {
            // Since we don't provide a payer, this transaction will end up
            // filtered at legacy.rs sanitize method (banking_stage) with error "a program cannot be payer"

            let instructions = vec![CompiledInstruction::new(
                0,
                &transfer_instruction,
                vec![0, 1],
            )];

            let keypairs = kpvals.unwrap();
            Transaction::new_with_compiled_instructions(
                &keypairs,
                &[],
                self.blockhash,
                program_ids,
                instructions,
            )
        } else {
            // Since we provided invalid signatures
            // this transaction will end up filtered at legacy.rs (banking_stage) because
            // num_required_signatures == 0
            let instructions = vec![CompiledInstruction::new(
                0,
                &transfer_instruction,
                vec![0, 1],
            )];

            let mut tx = Transaction::new_with_compiled_instructions(
                &[] as &[&Keypair; 0],
                &[],
                self.blockhash,
                program_ids,
                instructions,
            );
            tx.signatures = vec![Signature::new_unique(); self.transaction_params.num_signatures];
            tx
        }
    }
}

// Multithreading-related functions
//
// The most computationally expensive work is signing new
// transactions. So we generate them in n threads.
// Sending transactions is at least x8 times cheaper operation
// so we use only one thread for that for now:
//
// |TxGenerator|{n} -> |Tx channel|{1} -> |Sender|{1}
enum TransactionMsg {
    Transaction(Transaction),
    Exit,
}

fn create_sender_thread(
    tx_receiver: Receiver<TransactionMsg>,
    mut n_alive_threads: usize,
    target: &SocketAddr,
    addr: SocketAddr,
) -> thread::JoinHandle<()> {
    let socket = UdpSocket::bind("0.0.0.0:0").unwrap();
    let target = target.clone();
    let timer_receiver = tick(Duration::from_millis(REPORT_EACH_MILLIS as u64));

    let executor = TransactionExecutor::new(addr);

    thread::spawn(move || {
        let mut count: usize = 0;
        let mut total_count: usize = 0;
        let mut error_count = 0;
        let start_total = Instant::now();
        loop {
            select! {
                recv(tx_receiver) -> msg => {
                    match msg {
                        Ok(TransactionMsg::Transaction(tx)) => {
                            //let data = bincode::serialize(&tx).unwrap();
                            //let res = socket.send_to(&data, target);
                            //if res.is_err() {
                            //    error_count += 1;
                            //}
                            executor.push_transactions(vec![tx]);
                            let _ = executor.drain_cleared();

                            count += 1;
                            total_count += 1;
                        }
                        Ok(TransactionMsg::Exit) => {
                            info!("Worker is done");
                            n_alive_threads -= 1;
                            if n_alive_threads == 0 {
                                let t = start_total.elapsed().as_micros() as f64;
                                info!("Stopping sender. Count: {}, errors count: {}, total time: {}s, tps: {}",
                                    total_count,
                                    error_count,
                                    t / 1e6,
                                    ((count as f64)*1e6) / t,
                                );

                                break;
                            }
                        }
                        _ => panic!("Sender panics"),
                    }
                },
                recv(timer_receiver) -> _ => {
                    let t = start_total.elapsed().as_micros() as f64;
                    info!("Count: {}, tps: {}, time: {}",
                        count,
                        compute_tps(count),
                        t / 1e6,
                    );
                    count = 0;
                }
            }
        }
    })
}

// Keypair is not clonable, but we need to clone it to pass to threads
fn clone_payer(payer: &Option<&Keypair>) -> Option<Keypair> {
    if payer.is_none() {
        None
    } else {
        let bytes = payer.unwrap().to_bytes();
        match Keypair::from_bytes(&bytes) {
            Ok(kp) => Some(kp),
            Err(_) => None,
        }
    }
}
// TODO use Measure struct from bench instead of manual measuring

fn create_generator_thread<T: 'static + Client + Send + Sync>(
    tx_sender: &Sender<TransactionMsg>,
    max_iter_per_thread: usize,
    transaction_generator: &mut TransactionGenerator,
    funding_key: &Arc<Keypair>,
    client: &Arc<T>,
    faucet_addr: Option<SocketAddr>,
    payer: &Option<&Keypair>,
    rpc_client: &Arc<RpcClient>,
) -> thread::JoinHandle<()> {
    let tx_sender = tx_sender.clone();
    let mut transaction_generator = transaction_generator.clone();
    let rpc_client = rpc_client.clone(); // TODO remove later, use thread for blockhashes
    let payer = payer.clone();
    let client = client.clone();

    let num_signatures = transaction_generator.transaction_params.num_signatures;
    let valid_signatures = transaction_generator.transaction_params.valid_signatures;

    let payer = clone_payer(&payer);

    let mut keypairs_flat: Vec<Keypair> =
        generate_and_fund_keypairs(client, faucet_addr, funding_key, 10 * num_signatures, 100)
            .unwrap();
    info!("FINISHED");

    // Generate n=1000 unique keypairs, which are used to create
    // chunks of keypairs.
    // The number of chunks is described by binomial coefficient
    // and hence 1000 seems to be a reasonable choice
    //let mut keypairs_flat: Vec<Keypair> = Vec::new();
    //if valid_signatures {
    //    keypairs_flat = (0..1000 * num_signatures).map(|_| Keypair::new()).collect();
    //}

    // TODO, rewrite later: I'm not sure if we need to be precise about message cost etc
    /*let lamports_per_account = 10;
    let max_fee = 2; //client.get_fee_for_message(&single_sig_message).unwrap();
    let total_keypairs = keypairs_flat.len() as u64 + 1; // Add one for funding keypair
    let total = 100; //lamports_per_account * total_keypairs;
    fund_keys(
        client,
        funding_key,
        &keypairs_flat.as_slice(),
        total,
        max_fee,
        lamports_per_account,
    );*/

    thread::spawn(move || {
        let indexes: Vec<usize> = (0..keypairs_flat.len()).collect();
        let mut it = indexes.iter().permutations(num_signatures);
        let mut cnt = 0;
        let mut generation_elapsed: u64 = 0;
        loop {
            let generation_start = Instant::now();
            let chunk_keypairs = if valid_signatures {
                let permut = it.next();
                if permut.is_none() {
                    keypairs_flat.iter_mut().for_each(|v| *v = Keypair::new());
                    info!("Regenerate keypairs");
                    continue;
                }
                let permut = permut.unwrap();
                Some(apply_permutation(permut, &keypairs_flat))
            } else {
                None
            };

            // TODO clean up later
            let payer = if payer.is_none() {
                None
            } else {
                Some(payer.as_ref().unwrap())
            };
            let tx = transaction_generator.generate(payer, chunk_keypairs, &rpc_client);
            generation_elapsed =
                generation_elapsed.saturating_add(generation_start.elapsed().as_micros() as u64);

            let _ = tx_sender.send(TransactionMsg::Transaction(tx));
            cnt += 1;
            if max_iter_per_thread != 0 && cnt >= max_iter_per_thread {
                let _ = tx_sender.send(TransactionMsg::Exit);
                break;
            }
        }
        info!(
            "Finished thread. count = {}, avg generation time = {}, tps = {}",
            cnt,
            generation_elapsed / 1_000,
            (cnt as f64) / (generation_elapsed as f64) * 1e6
        );
    })
}

fn get_target_and_client(
    nodes: &[ContactInfo],
    mode: Mode,
    entrypoint_addr: SocketAddr,
) -> (Option<SocketAddr>, Option<RpcClient>) {
    let mut target = None;
    let mut rpc_client = None;
    if nodes.is_empty() {
        if mode == Mode::Rpc {
            rpc_client = Some(RpcClient::new_socket(entrypoint_addr));
        }
        target = Some(entrypoint_addr);
    } else {
        info!("************ NODE ***********");
        for node in nodes {
            info!("{:?}", node);
        }
        info!("ADDR = {}", entrypoint_addr);

        for node in nodes {
            if node.gossip == entrypoint_addr {
                info!("{}", node.gossip);
                target = match mode {
                    Mode::Gossip => Some(node.gossip),
                    Mode::Tvu => Some(node.tvu),
                    Mode::TvuForwards => Some(node.tvu_forwards),
                    Mode::Tpu => {
                        rpc_client = Some(RpcClient::new_socket(node.rpc));
                        Some(node.tpu)
                    }
                    Mode::TpuForwards => Some(node.tpu_forwards),
                    Mode::Repair => Some(node.repair),
                    Mode::ServeRepair => Some(node.serve_repair),
                    Mode::Rpc => {
                        rpc_client = Some(RpcClient::new_socket(node.rpc));
                        None
                    }
                };
                break;
            }
        }
    }
    (target, rpc_client)
}

fn run_dos_rpc_mode(
    rpc_client: Option<RpcClient>,
    iterations: usize,
    data_type: DataType,
    data_input: Option<String>,
) {
    let mut last_log = Instant::now();
    let mut total_count: usize = 0;
    let mut count = 0;
    let mut error_count = 0;
    loop {
        match data_type {
            DataType::GetAccountInfo => {
                let res = rpc_client
                    .as_ref()
                    .unwrap()
                    .get_account(&Pubkey::from_str(data_input.as_ref().unwrap()).unwrap());
                if res.is_err() {
                    error_count += 1;
                }
            }
            DataType::GetProgramAccounts => {
                let res = rpc_client
                    .as_ref()
                    .unwrap()
                    .get_program_accounts(&Pubkey::from_str(data_input.as_ref().unwrap()).unwrap());
                if res.is_err() {
                    error_count += 1;
                }
            }
            _ => {
                panic!("unsupported data type");
            }
        }
        count += 1;
        total_count += 1;
        if last_log.elapsed().as_millis() > REPORT_EACH_MILLIS {
            info!(
                "count: {}, errors: {}, tps: {}",
                count,
                error_count,
                compute_tps(count)
            );
            last_log = Instant::now();
            count = 0;
        }
        if iterations != 0 && total_count >= iterations {
            break;
        }
    }
}

fn apply_permutation<'a, T>(indexes: Vec<&usize>, items: &'a Vec<T>) -> Vec<&'a T> {
    let mut res = Vec::with_capacity(indexes.len());
    for i in indexes {
        res.push(&items[*i]);
    }
    res
}

fn run_dos_transactions<T: 'static + Client + Send + Sync>(
    addr: SocketAddr,
    rpc_client: Option<RpcClient>,
    target: SocketAddr,
    iterations: usize,
    funding_key: Keypair,
    client: Arc<T>,
    faucet_addr: Option<SocketAddr>,
    payer: Option<&Keypair>,
    transaction_params: TransactionParams,
) {
    let rpc_client = Arc::new(rpc_client.unwrap());
    let payer = Arc::new(payer);
    info!("{:?}", transaction_params);
    let num_gen_threads = transaction_params.num_gen_threads;
    let mut transaction_generator = TransactionGenerator::new(transaction_params);

    let (tx_sender, tx_receiver) = unbounded();

    let sender_thread = create_sender_thread(tx_receiver, num_gen_threads, &target, addr);

    let max_iter_per_thread = iterations / num_gen_threads;

    let funding_key = Arc::new(funding_key);
    let rpc_client = Arc::new(rpc_client); // TODO Replace rpc client with ThinClient since it provides more options
    let tx_generator_threads: Vec<_> = (0..num_gen_threads)
        .into_iter()
        .map(|_| {
            create_generator_thread(
                &tx_sender,
                max_iter_per_thread,
                &mut transaction_generator,
                &funding_key,
                &client,
                faucet_addr,
                &payer,
                &rpc_client,
            )
        })
        .collect();

    if let Err(err) = sender_thread.join() {
        println!("join() failed with: {:?}", err);
    }
    for t_generator in tx_generator_threads {
        if let Err(err) = t_generator.join() {
            println!("join() failed with: {:?}", err);
        }
    }
    println!("This is the end");
}

fn run_dos<T: 'static + Client + Send + Sync>(
    nodes: &[ContactInfo],
    iterations: usize,
    faucet_addr: Option<SocketAddr>,
    client: Arc<T>,
    payer: Option<&Keypair>,
    params: DosClientParameters,
) {
    let (target, rpc_client) = get_target_and_client(nodes, params.mode, params.entrypoint_addr);
    let target = target.expect("should have target");
    info!("Targeting {}", target);

    if params.mode == Mode::Rpc {
        run_dos_rpc_mode(rpc_client, iterations, params.data_type, params.data_input);
    } else if params.data_type == DataType::Transaction
        && params.transaction_params.unique_transactions
    {
        let total = 100_000_000;
        let funding_key = Keypair::new();
        //if client.get_balance(&funding_key.pubkey()).unwrap_or(0) < total {
        //    let r = airdrop_lamports(client.as_ref(), faucet_addr.as_ref().unwrap(), &funding_key, total);
        //    match r {
        //        Ok(_) => {},
        //        Err(x) => println!("{:?}", x)
        //    }
        //}

        let addr = nodes[0].rpc;

        run_dos_transactions(
            addr,
            rpc_client,
            target,
            iterations,
            funding_key,
            client,
            faucet_addr,
            payer,
            params.transaction_params,
        );
    } else {
        let mut data = match params.data_type {
            DataType::RepairHighest => {
                let slot = 100;
                let req =
                    RepairProtocol::WindowIndexWithNonce(get_repair_contact(nodes), slot, 0, 0);
                bincode::serialize(&req).unwrap()
            }
            DataType::RepairShred => {
                let slot = 100;
                let req = RepairProtocol::HighestWindowIndexWithNonce(
                    get_repair_contact(nodes),
                    slot,
                    0,
                    0,
                );
                bincode::serialize(&req).unwrap()
            }
            DataType::RepairOrphan => {
                let slot = 100;
                let req = RepairProtocol::OrphanWithNonce(get_repair_contact(nodes), slot, 0);
                bincode::serialize(&req).unwrap()
            }
            DataType::Random => {
                vec![0; params.data_size]
            }
            DataType::Transaction => {
                let tp = params.transaction_params;
                info!("{:?}", tp);

                let mut transaction_generator = TransactionGenerator::new(tp);
                let rpc_client = Arc::new(rpc_client.unwrap());
                let tx = transaction_generator.generate(payer, None, &rpc_client);
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
            let res = socket.send_to(&data, target);
            if res.is_err() {
                error_count += 1;
            }
            count += 1;
            total_count += 1;
            if last_log.elapsed().as_millis() > REPORT_EACH_MILLIS {
                info!(
                    "count: {}, errors: {}, tps: {}",
                    count,
                    error_count,
                    compute_tps(count)
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

    let mut nodes = vec![];
    if !cmd_params.skip_gossip {
        info!("Finding cluster entry: {:?}", cmd_params.entrypoint_addr);
        let socket_addr_space = SocketAddrSpace::new(cmd_params.allow_private_addr);
        let (gossip_nodes, _validators) = discover(
            None, // keypair
            Some(&cmd_params.entrypoint_addr),
            None,                              // num_nodes
            Duration::from_secs(60),           // timeout
            None,                              // find_node_by_pubkey
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
        nodes = gossip_nodes;
    }

    info!("done found {} nodes", nodes.len());
    let payer = cmd_params
        .transaction_params
        .payer_filename
        .as_ref()
        .map(|keypair_file_name| {
            read_keypair_file(&keypair_file_name)
                .unwrap_or_else(|_| panic!("bad keypair {:?}", keypair_file_name))
        });

    //run_dos(&nodes, 0, None, payer.as_ref(), cmd_params);
}

#[cfg(test)]
pub mod test {
    use solana_sdk::commitment_config::CommitmentConfig;
    use {
        super::*,
        solana_client::thin_client::create_client,
        solana_core::validator::ValidatorConfig,
        solana_faucet::faucet::run_local_faucet,
        solana_gossip::cluster_info::VALIDATOR_PORT_RANGE,
        solana_local_cluster::{
            cluster::Cluster,
            local_cluster::{ClusterConfig, LocalCluster},
            validator_configs::make_identical_validator_configs,
        },
        solana_sdk::timing::timestamp,
    };

    /*
        #[test]
        fn test_dos() {
            let nodes = [ContactInfo::new_localhost(
                &solana_sdk::pubkey::new_rand(),
                timestamp(),
            )];
            let entrypoint_addr = nodes[0].gossip;

            run_dos(
                &nodes,
                1,
                None,
                DosClientParameters {
                    entrypoint_addr,
                    mode: Mode::Tvu,
                    data_size: 10,
                    data_type: DataType::Random,
                    data_input: None,
                    skip_gossip: false,
                    allow_private_addr: false,
                    transaction_params: TransactionParams::default(),
                },
            );

            run_dos(
                &nodes,
                1,
                None,
                DosClientParameters {
                    entrypoint_addr,
                    mode: Mode::Repair,
                    data_size: 10,
                    data_type: DataType::RepairHighest,
                    data_input: None,
                    skip_gossip: false,
                    allow_private_addr: false,
                    transaction_params: TransactionParams::default(),
                },
            );

            run_dos(
                &nodes,
                1,
                None,
                DosClientParameters {
                    entrypoint_addr,
                    mode: Mode::ServeRepair,
                    data_size: 10,
                    data_type: DataType::RepairShred,
                    data_input: None,
                    skip_gossip: false,
                    allow_private_addr: false,
                    transaction_params: TransactionParams::default(),
                },
            );
        }

        #[test]
        #[ignore]
        fn test_dos_local_cluster_transactions() {
            let num_nodes = 1;
            let cluster =
                LocalCluster::new_with_equal_stakes(num_nodes, 100, 3, SocketAddrSpace::Unspecified);
            assert_eq!(cluster.validators.len(), num_nodes);

            let nodes = cluster.get_node_pubkeys();
            let node = cluster.get_contact_info(&nodes[0]).unwrap().clone();
            let nodes_slice = [node];

            // send random transactions to TPU
            // will be discarded on sigverify stage
            run_dos(
                &nodes_slice,
                1,
                None,
                DosClientParameters {
                    entrypoint_addr: cluster.entry_point_info.gossip,
                    mode: Mode::Tpu,
                    data_size: 1024,
                    data_type: DataType::Random,
                    data_input: None,
                    skip_gossip: false,
                    allow_private_addr: false,
                    transaction_params: TransactionParams::default(),
                },
            );

            // send transactions to TPU with 2 random signatures
            // will be filtered on dedup (because transactions are not unique)
            run_dos(
                &nodes_slice,
                1,
                None,
                DosClientParameters {
                    entrypoint_addr: cluster.entry_point_info.gossip,
                    mode: Mode::Tpu,
                    data_size: 0, // irrelevant if not random
                    data_type: DataType::Transaction,
                    data_input: None,
                    skip_gossip: false,
                    allow_private_addr: false,
                    transaction_params: TransactionParams {
                        num_signatures: 2,
                        valid_blockhash: false,
                        valid_signatures: false,
                        unique_transactions: false,
                        payer_filename: None,
                        num_gen_threads: 1,
                    },
                },
            );

            // send *unique* transactions to TPU with 4 random signatures
            // will be discarded on banking stage in legacy.rs
            // ("there should be at least 1 RW fee-payer account")
            run_dos(
                &nodes_slice,
                1,
                None,
                DosClientParameters {
                    entrypoint_addr: cluster.entry_point_info.gossip,
                    mode: Mode::Tpu,
                    data_size: 0, // irrelevant if not random
                    data_type: DataType::Transaction,
                    data_input: None,
                    skip_gossip: false,
                    allow_private_addr: false,
                    transaction_params: TransactionParams {
                        num_signatures: 4,
                        valid_blockhash: false,
                        valid_signatures: false,
                        unique_transactions: true,
                        payer_filename: None,
                        num_gen_threads: 1,
                    },
                },
            );

            // send unique transactions to TPU with 2 random signatures
            // will be discarded on banking stage in legacy.rs (A program cannot be a payer)
            // because we haven't provided a valid payer
            run_dos(
                &nodes_slice,
                1,
                None,
                DosClientParameters {
                    entrypoint_addr: cluster.entry_point_info.gossip,
                    mode: Mode::Tpu,
                    data_size: 0, // irrelevant if not random
                    data_type: DataType::Transaction,
                    data_input: None,
                    skip_gossip: false,
                    allow_private_addr: false,
                    transaction_params: TransactionParams {
                        num_signatures: 2,
                        valid_blockhash: false, // irrelevant without valid payer, because
                        // it will be filtered before blockhash validity checks
                        valid_signatures: true,
                        unique_transactions: true,
                        payer_filename: None,
                        num_gen_threads: 1,
                    },
                },
            );

            // send unique transaction to TPU with valid blockhash
            // will be discarded due to invalid hash
            run_dos(
                &nodes_slice,
                1,
                Some(&cluster.funding_keypair),
                DosClientParameters {
                    entrypoint_addr: cluster.entry_point_info.gossip,
                    mode: Mode::Tpu,
                    data_size: 0, // irrelevant if not random
                    data_type: DataType::Transaction,
                    data_input: None,
                    skip_gossip: false,
                    allow_private_addr: false,
                    transaction_params: TransactionParams {
                        num_signatures: 2,
                        valid_blockhash: false,
                        valid_signatures: true,
                        unique_transactions: true,
                        payer_filename: None,
                        num_gen_threads: 1,
                    },
                },
            );

            // send unique transaction to TPU with valid blockhash
            // will fail with error processing Instruction 0: missing required signature for instruction
            run_dos(
                &nodes_slice,
                1,
                Some(&cluster.funding_keypair),
                DosClientParameters {
                    entrypoint_addr: cluster.entry_point_info.gossip,
                    mode: Mode::Tpu,
                    data_size: 0, // irrelevant if not random
                    data_type: DataType::Transaction,
                    data_input: None,
                    skip_gossip: false,
                    allow_private_addr: false,
                    transaction_params: TransactionParams {
                        num_signatures: 2,
                        valid_blockhash: true,
                        valid_signatures: true,
                        unique_transactions: true,
                        payer_filename: None,
                        num_gen_threads: 1,
                    },
                },
            );
        }

        #[test]
        #[ignore]
        fn test_dos_local_cluster() {
            solana_logger::setup();
            let num_nodes = 1;
            let cluster =
                LocalCluster::new_with_equal_stakes(num_nodes, 100, 3, SocketAddrSpace::Unspecified);
            assert_eq!(cluster.validators.len(), num_nodes);

            let nodes = cluster.get_node_pubkeys();
            let node = cluster.get_contact_info(&nodes[0]).unwrap().clone();

            run_dos(
                &[node],
                10_000_000,
                Some(&cluster.funding_keypair),
                DosClientParameters {
                    entrypoint_addr: cluster.entry_point_info.gossip,
                    mode: Mode::Tpu,
                    data_size: 0, // irrelevant if not random
                    data_type: DataType::Transaction,
                    data_input: None,
                    skip_gossip: false,
                    allow_private_addr: false,
                    transaction_params: TransactionParams {
                        num_signatures: 2,
                        valid_blockhash: true,
                        valid_signatures: true,
                        unique_transactions: true,
                        payer_filename: None,
                        num_gen_threads: 1,
                    },
                },
            );
        }

        #[test]
        #[ignore]
        fn test_dos_random() {
            solana_logger::setup();
            let num_nodes = 1;
            let cluster =
                LocalCluster::new_with_equal_stakes(num_nodes, 100, 3, SocketAddrSpace::Unspecified);
            assert_eq!(cluster.validators.len(), num_nodes);

            let nodes = cluster.get_node_pubkeys();
            let node = cluster.get_contact_info(&nodes[0]).unwrap().clone();
            let nodes_slice = [node];

            // send random transactions to TPU
            // will be discarded on sigverify stage
            run_dos(
                &nodes_slice,
                100000,
                None,
                DosClientParameters {
                    entrypoint_addr: cluster.entry_point_info.gossip,
                    mode: Mode::Tpu,
                    data_size: 1024,
                    data_type: DataType::Random,
                    data_input: None,
                    skip_gossip: false,
                    allow_private_addr: false,
                    transaction_params: TransactionParams::default(),
                },
            );
        }
    */
    #[test]
    fn test_dos_unique() {
        solana_logger::setup();
        let num_nodes = 1;
        //let cluster =
        //    LocalCluster::new_with_equal_stakes(num_nodes, 200_000_000, 3, SocketAddrSpace::Unspecified);
        let native_instruction_processors = vec![];
        let cluster = LocalCluster::new(
            &mut ClusterConfig {
                node_stakes: vec![999_990; num_nodes],
                cluster_lamports: 200_000_000,
                validator_configs: make_identical_validator_configs(
                    &ValidatorConfig::default_for_test(),
                    num_nodes,
                ),
                native_instruction_processors,
                ..ClusterConfig::default()
            },
            SocketAddrSpace::Unspecified,
        );
        assert_eq!(cluster.validators.len(), num_nodes);

        /// 1. Transfer funds to faucet account
        /// 2. Create faucet thread
        /// 3. Fund funding_key using faucet
        /// 4. Transfer required funds from funding_key account to newly created accounts
        // create faucet keypair and fund it
        let faucet_keypair = Keypair::new();
        cluster.transfer(
            &cluster.funding_keypair,
            &faucet_keypair.pubkey(),
            100_000_000,
        );
        //
        let faucet_addr = run_local_faucet(faucet_keypair, None);

        let nodes = cluster.get_node_pubkeys();
        let node = cluster.get_contact_info(&nodes[0]).unwrap().clone();
        let nodes_slice = [node];

        // compute amount needed, for now just set
        // create payer and give it all the money
        let client = Arc::new(create_client(
            (cluster.entry_point_info.rpc, cluster.entry_point_info.tpu),
            VALIDATOR_PORT_RANGE,
        ));

        //let funding_key = Keypair::new();
        //let keypair_count = 2000;
        //let keypairs = generate_and_fund_keypairs(
        //    client.clone(),
        //    Some(faucet_addr),
        //    &funding_key,
        //    keypair_count,
        //    100,
        //)
        //.unwrap();

        // send unique transaction to TPU with valid blockhash
        // will fail with error processing Instruction 0: missing required signature for instruction
        run_dos(
            &nodes_slice,
            10,
            Some(faucet_addr),
            client,
            Some(&cluster.funding_keypair),
            DosClientParameters {
                entrypoint_addr: cluster.entry_point_info.gossip,
                mode: Mode::Tpu,
                data_size: 0, // irrelevant if not random
                data_type: DataType::Transaction,
                data_input: None,
                skip_gossip: false,
                allow_private_addr: false,
                transaction_params: TransactionParams {
                    num_signatures: 2,
                    valid_blockhash: true,
                    valid_signatures: true,
                    unique_transactions: true,
                    payer_filename: None,
                    num_gen_threads: 1,
                },
            },
        );
    }
}
