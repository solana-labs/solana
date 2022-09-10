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
    clap::{crate_description, crate_name, crate_version, ArgEnum, Args, Parser},
    log::*,
    rand::{thread_rng, Rng},
    serde::{Deserialize, Serialize},
    solana_client::rpc_client::RpcClient,
    solana_core::serve_repair::{RepairProtocol, RepairRequestHeader, ServeRepair},
    solana_gossip::{contact_info::ContactInfo, gossip_service::discover},
    solana_sdk::{
        hash::Hash,
        instruction::{AccountMeta, CompiledInstruction, Instruction},
        pubkey::Pubkey,
        signature::{read_keypair_file, Keypair, Signature, Signer},
        stake,
        system_instruction::SystemInstruction,
        system_program,
        timing::timestamp,
        transaction::Transaction,
    },
    solana_streamer::socket::SocketAddrSpace,
    std::{
        net::{SocketAddr, UdpSocket},
        process::exit,
        str::FromStr,
        time::{Duration, Instant},
    },
};

struct TransactionGenerator {
    blockhash: Hash,
    last_generated: Instant,
    transaction_params: TransactionParams,
    cached_transaction: Option<Transaction>,
}

impl TransactionGenerator {
    fn new(transaction_params: TransactionParams) -> Self {
        TransactionGenerator {
            blockhash: Hash::default(),
            last_generated: (Instant::now() - Duration::from_secs(100)),
            transaction_params,
            cached_transaction: None,
        }
    }

    fn generate(&mut self, payer: Option<&Keypair>, rpc_client: &Option<RpcClient>) -> Transaction {
        if !self.transaction_params.unique_transactions && self.cached_transaction.is_some() {
            return self.cached_transaction.as_ref().unwrap().clone();
        }

        // generate a new blockhash every 1sec
        if self.transaction_params.valid_blockhash
            && self.last_generated.elapsed().as_millis() > 1000
        {
            self.blockhash = rpc_client.as_ref().unwrap().get_latest_blockhash().unwrap();
            self.last_generated = Instant::now();
        }

        // in order to evaluate the performance implications of the different transactions
        // we create here transactions which are filtered out on different stages of processing pipeline

        // create an arbitrary valid instruction
        let lamports = 5;
        let transfer_instruction = SystemInstruction::Transfer { lamports };
        let program_ids = vec![system_program::id(), stake::program::id()];

        // transaction with payer, in this case signatures are valid and num_signatures is irrelevant
        // random payer will cause error "attempt to debit an account but found no record of a prior credit"
        // if payer is correct, it will trigger error with not enough signatures
        let transaction = if let Some(payer) = payer {
            let instruction = Instruction::new_with_bincode(
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
            )
        } else if self.transaction_params.valid_signatures {
            // Since we don't provide a payer, this transaction will end up
            // filtered at legacy.rs sanitize method (banking_stage) with error "a program cannot be payer"
            let kpvals: Vec<Keypair> = (0..self.transaction_params.num_signatures)
                .map(|_| Keypair::new())
                .collect();
            let keypairs: Vec<&Keypair> = kpvals.iter().collect();

            let instructions = vec![CompiledInstruction::new(
                0,
                &transfer_instruction,
                vec![0, 1],
            )];

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
        };

        // if we need to generate only one transaction, we cache it to reuse later
        if !self.transaction_params.unique_transactions {
            self.cached_transaction = Some(transaction.clone());
        }

        transaction
    }
}

fn run_dos(
    nodes: &[ContactInfo],
    iterations: usize,
    payer: Option<&Keypair>,
    params: DosClientParameters,
) {
    let mut target = None;
    let mut rpc_client = None;
    if nodes.is_empty() {
        if params.mode == Mode::Rpc {
            rpc_client = Some(RpcClient::new_socket(params.entrypoint_addr));
        }
        target = Some((solana_sdk::pubkey::new_rand(), params.entrypoint_addr));
    } else {
        info!("************ NODE ***********");
        for node in nodes {
            info!("{:?}", node);
        }
        info!("ADDR = {}", params.entrypoint_addr);

        for node in nodes {
            if node.gossip == params.entrypoint_addr {
                info!("{}", node.gossip);
                target = match params.mode {
                    Mode::Gossip => Some((node.id, node.gossip)),
                    Mode::Tvu => Some((node.id, node.tvu)),
                    Mode::TvuForwards => Some((node.id, node.tvu_forwards)),
                    Mode::Tpu => {
                        rpc_client = Some(RpcClient::new_socket(node.rpc));
                        Some((node.id, node.tpu))
                    }
                    Mode::TpuForwards => Some((node.id, node.tpu_forwards)),
                    Mode::Repair => Some((node.id, node.repair)),
                    Mode::ServeRepair => Some((node.id, node.serve_repair)),
                    Mode::Rpc => {
                        rpc_client = Some(RpcClient::new_socket(node.rpc));
                        None
                    }
                };
                break;
            }
        }
    }
    let (target_id, target_addr) = target.expect("should have target");
    info!("Targeting {}", target_addr);
    let socket = UdpSocket::bind("0.0.0.0:0").unwrap();

    let mut data = Vec::new();
    let mut transaction_generator = None;

    match params.data_type {
        DataType::RepairHighest => {
            let slot = 100;
            let keypair = Keypair::new();
            let header = RepairRequestHeader::new(keypair.pubkey(), target_id, timestamp(), 0);
            let req = RepairProtocol::WindowIndex {
                header,
                slot,
                shred_index: 0,
            };
            data = ServeRepair::repair_proto_to_bytes(&req, Some(&keypair)).unwrap();
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
            data = ServeRepair::repair_proto_to_bytes(&req, Some(&keypair)).unwrap();
        }
        DataType::RepairOrphan => {
            let slot = 100;
            let keypair = Keypair::new();
            let header = RepairRequestHeader::new(keypair.pubkey(), target_id, timestamp(), 0);
            let req = RepairProtocol::Orphan { header, slot };
            data = ServeRepair::repair_proto_to_bytes(&req, Some(&keypair)).unwrap();
        }
        DataType::Random => {
            data.resize(params.data_size, 0);
        }
        DataType::Transaction => {
            let tp = params.transaction_params;
            info!("{:?}", tp);

            transaction_generator = Some(TransactionGenerator::new(tp));
            let tx = transaction_generator
                .as_mut()
                .unwrap()
                .generate(payer, &rpc_client);
            info!("{:?}", tx);
            data = bincode::serialize(&tx).unwrap();
        }
        DataType::GetAccountInfo => {}
        DataType::GetProgramAccounts => {}
    }

    let mut last_log = Instant::now();
    let mut count = 0;
    let mut error_count = 0;
    loop {
        if params.mode == Mode::Rpc {
            match params.data_type {
                DataType::GetAccountInfo => {
                    let res = rpc_client.as_ref().unwrap().get_account(
                        &Pubkey::from_str(params.data_input.as_ref().unwrap()).unwrap(),
                    );
                    if res.is_err() {
                        error_count += 1;
                    }
                }
                DataType::GetProgramAccounts => {
                    let res = rpc_client.as_ref().unwrap().get_program_accounts(
                        &Pubkey::from_str(params.data_input.as_ref().unwrap()).unwrap(),
                    );
                    if res.is_err() {
                        error_count += 1;
                    }
                }
                _ => {
                    panic!("unsupported data type");
                }
            }
        } else {
            if params.data_type == DataType::Random {
                thread_rng().fill(&mut data[..]);
            }
            if let Some(tg) = transaction_generator.as_mut() {
                let tx = tg.generate(payer, &rpc_client);
                info!("{:?}", tx);
                data = bincode::serialize(&tx).unwrap();
            }
            let res = socket.send_to(&data, target_addr);
            if res.is_err() {
                error_count += 1;
            }
        }
        count += 1;
        if last_log.elapsed().as_millis() > 10_000 {
            info!("count: {} errors: {}", count, error_count);
            last_log = Instant::now();
            count = 0;
        }
        if iterations != 0 && count >= iterations {
            break;
        }
    }
}

// command line parsing
#[derive(Parser)]
#[clap(name = crate_name!(),
    version = crate_version!(),
    about = crate_description!(),
    rename_all = "kebab-case"
)]
struct DosClientParameters {
    #[clap(long, arg_enum, help = "Interface to DoS")]
    mode: Mode,

    #[clap(long, arg_enum, help = "Type of data to send")]
    data_type: DataType,

    #[clap(
        long = "entrypoint",
        parse(try_from_str = addr_parser),
        default_value = "127.0.0.1:8001",
        help = "Gossip entrypoint address. Usually <ip>:8001"
    )]
    entrypoint_addr: SocketAddr,

    #[clap(
        long,
        default_value = "128",
        required_if_eq("data-type", "random"),
        help = "Size of packet to DoS with, relevant only for data-type=random"
    )]
    data_size: usize,

    #[clap(long, help = "Data to send [Optional]")]
    data_input: Option<String>,

    #[clap(long, help = "Just use entrypoint address directly")]
    skip_gossip: bool,

    #[clap(long, help = "Allow contacting private ip addresses")]
    allow_private_addr: bool,

    #[clap(flatten)]
    transaction_params: TransactionParams,
}

#[derive(Args, Serialize, Deserialize, Debug, Default)]
#[clap(rename_all = "kebab-case")]
struct TransactionParams {
    #[clap(
        long,
        default_value = "2",
        help = "Number of signatures in transaction"
    )]
    num_signatures: usize,

    #[clap(long, help = "Generate a valid blockhash for transaction")]
    valid_blockhash: bool,

    #[clap(long, help = "Generate valid signature(s) for transaction")]
    valid_signatures: bool,

    #[clap(long, help = "Generate unique transactions")]
    unique_transactions: bool,

    #[clap(
        long = "payer",
        help = "Payer's keypair file to fund transactions [Optional]"
    )]
    payer_filename: Option<String>,
}

#[derive(ArgEnum, Clone, Eq, PartialEq)]
enum Mode {
    Gossip,
    Tvu,
    TvuForwards,
    Tpu,
    TpuForwards,
    Repair,
    ServeRepair,
    Rpc,
}

#[derive(ArgEnum, Clone, Eq, PartialEq)]
enum DataType {
    RepairHighest,
    RepairShred,
    RepairOrphan,
    Random,
    GetAccountInfo,
    GetProgramAccounts,
    Transaction,
}

fn addr_parser(addr: &str) -> Result<SocketAddr, &'static str> {
    match solana_net_utils::parse_host_port(addr) {
        Ok(v) => Ok(v),
        Err(_) => Err("failed to parse entrypoint address"),
    }
}

/// input checks which are not covered by Clap
fn validate_input(params: &DosClientParameters) {
    if params.mode == Mode::Rpc
        && (params.data_type != DataType::GetAccountInfo
            && params.data_type != DataType::GetProgramAccounts)
    {
        panic!("unsupported data type");
    }

    if params.data_type != DataType::Transaction {
        let tp = &params.transaction_params;
        if tp.valid_blockhash
            || tp.valid_signatures
            || tp.unique_transactions
            || tp.payer_filename.is_some()
        {
            println!("Arguments valid-blockhash, valid-sign, unique-trans, payer are ignored if data-type != transaction");
        }
    }

    if params.transaction_params.payer_filename.is_some()
        && params.transaction_params.valid_signatures
    {
        println!("Arguments valid-signatures is ignored if payer is provided");
    }
}

fn main() {
    solana_logger::setup_with_default("solana=info");
    let cmd_params = DosClientParameters::parse();
    validate_input(&cmd_params);

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

    run_dos(&nodes, 0, payer.as_ref(), cmd_params);
}

#[cfg(test)]
pub mod test {
    use {
        super::*,
        solana_local_cluster::{cluster::Cluster, local_cluster::LocalCluster},
        solana_sdk::timing::timestamp,
    };

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
                },
            },
        );
    }
}
