#![allow(clippy::integer_arithmetic)]
use {
    clap::{crate_description, crate_name, value_t, value_t_or_exit, App, Arg},
    log::*,
    rand::{thread_rng, Rng},
    serde::{Deserialize, Serialize},
    solana_client::rpc_client::RpcClient,
    solana_core::serve_repair::RepairProtocol,
    solana_gossip::{contact_info::ContactInfo, gossip_service::discover},
    solana_sdk::{
        hash::Hash,
        instruction::{AccountMeta, CompiledInstruction, Instruction},
        pubkey::Pubkey,
        signature::{read_keypair_file, Keypair, Signer},
        stake,
        system_instruction::SystemInstruction,
        system_program,
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

fn get_repair_contact(nodes: &[ContactInfo]) -> ContactInfo {
    let source = thread_rng().gen_range(0, nodes.len());
    let mut contact = nodes[source].clone();
    contact.id = solana_sdk::pubkey::new_rand();
    contact
}

/// Options for data_type=transaction
#[derive(Serialize, Deserialize, Debug)]
struct TransactionParams {
    unique_transactions: bool, // use unique transactions
    num_sign: usize,           // number of signatures in a transaction
    valid_block_hash: bool,    // use valid blockhash or random
    valid_signatures: bool,    // use valid signatures or not
    with_payer: bool,          // provide a valid payer
}

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

    fn generate(&mut self, payer: &Keypair, rpc_client: &Option<RpcClient>) -> Transaction {
        if !self.transaction_params.unique_transactions && self.cached_transaction != None {
            return self.cached_transaction.as_ref().unwrap().clone();
        }

        // generate a new blockhash every 1sec
        if self.transaction_params.valid_block_hash
            && self.last_generated.elapsed().as_millis() > 1000
        {
            self.blockhash = rpc_client.as_ref().unwrap().get_latest_blockhash().unwrap();
            self.last_generated = Instant::now();
        }

        let lamports = 5;
        let transfer_instruction = SystemInstruction::Transfer { lamports };
        let program_ids = vec![system_program::id(), stake::program::id()];

        // transaction with payer, in this case signatures are valid and num_sign is irrelevant
        // random payer will cause error "attempt to debit an account but found not record of a prior credit"
        // if payer is correct, it will trigger error with not enough signatures
        let transaction = if self.transaction_params.with_payer {
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
            // this way it wil end up filtered at legacy.rs#L217 (banking_stage)
            // with error "a program cannot be payer"
            let kpvals: Vec<Keypair> = (0..self.transaction_params.num_sign)
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
            // it will be filtered on the sigverify_stage
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
            tx.signatures =
                vec![Transaction::get_invalid_signature(); self.transaction_params.num_sign];
            tx
        };

        // if we need to generate only ony transaction, we cache it to reuse later
        if !self.transaction_params.unique_transactions {
            self.cached_transaction = Some(transaction.clone());
        }

        transaction
    }
}

fn run_dos(
    payer: &Keypair,
    nodes: &[ContactInfo],
    iterations: usize,
    entrypoint_addr: SocketAddr,
    data_type: String,
    data_size: usize,
    mode: String,
    data_input: Option<String>,
    transaction_params: Option<TransactionParams>,
) {
    let mut target = None;
    let mut rpc_client = None;
    if nodes.is_empty() {
        if mode == "rpc" {
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
            //let node = &nodes[1];
            if node.gossip == entrypoint_addr {
                info!("{}", node.gossip);
                target = match mode.as_str() {
                    "gossip" => Some(node.gossip),
                    "tvu" => Some(node.tvu),
                    "tvu_forwards" => Some(node.tvu_forwards),
                    "tpu" => {
                        rpc_client = Some(RpcClient::new_socket(node.rpc));
                        Some(node.tpu)
                    }
                    "tpu_forwards" => Some(node.tpu_forwards),
                    "repair" => Some(node.repair),
                    "serve_repair" => Some(node.serve_repair),
                    "rpc" => {
                        rpc_client = Some(RpcClient::new_socket(node.rpc));
                        None
                    }
                    &_ => panic!("Unknown mode"),
                };
                break;
            }
        }
    }
    let target = target.expect("should have target");

    info!("Targeting {}", target);
    let socket = UdpSocket::bind("0.0.0.0:0").unwrap();

    let mut data = Vec::new();
    let mut trans_gen = None;

    match data_type.as_str() {
        "repair_highest" => {
            let slot = 100;
            let req = RepairProtocol::WindowIndexWithNonce(get_repair_contact(nodes), slot, 0, 0);
            data = bincode::serialize(&req).unwrap();
        }
        "repair_shred" => {
            let slot = 100;
            let req =
                RepairProtocol::HighestWindowIndexWithNonce(get_repair_contact(nodes), slot, 0, 0);
            data = bincode::serialize(&req).unwrap();
        }
        "repair_orphan" => {
            let slot = 100;
            let req = RepairProtocol::OrphanWithNonce(get_repair_contact(nodes), slot, 0);
            data = bincode::serialize(&req).unwrap();
        }
        "random" => {
            data.resize(data_size, 0);
        }
        "transaction" => {
            if transaction_params.is_none() {
                panic!("transaction parameters are not specified");
            }
            let tp = transaction_params.unwrap();
            info!("{:?}", tp);

            trans_gen = Some(TransactionGenerator::new(tp));
            let tx = trans_gen.as_mut().unwrap().generate(payer, &rpc_client);
            info!("{:?}", tx);
            data = bincode::serialize(&tx).unwrap();
        }
        "get_account_info" => {}
        "get_program_accounts" => {}
        &_ => {
            panic!("unknown data type");
        }
    }

    info!("TARGET = {}, NODE = {}", target, nodes[1].rpc);

    let mut last_log = Instant::now();
    let mut count = 0;
    let mut error_count = 0;
    loop {
        if mode == "rpc" {
            match data_type.as_str() {
                "get_account_info" => {
                    let res = rpc_client
                        .as_ref()
                        .unwrap()
                        .get_account(&Pubkey::from_str(data_input.as_ref().unwrap()).unwrap());
                    if res.is_err() {
                        error_count += 1;
                    }
                }
                "get_program_accounts" => {
                    let res = rpc_client.as_ref().unwrap().get_program_accounts(
                        &Pubkey::from_str(data_input.as_ref().unwrap()).unwrap(),
                    );
                    if res.is_err() {
                        error_count += 1;
                    }
                }
                &_ => {
                    panic!("unsupported data type");
                }
            }
        } else {
            if data_type == "random" {
                thread_rng().fill(&mut data[..]);
            }
            if let Some(tg) = trans_gen.as_mut() {
                let tx = tg.generate(payer, &rpc_client);
                info!("{:?}", tx);
                data = bincode::serialize(&tx).unwrap();
            }
            let res = socket.send_to(&data, target);
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
                .help("Gossip entrypoint address. Usually <ip>:8001"),
        )
        .arg(
            Arg::with_name("mode")
                .long("mode")
                .takes_value(true)
                .value_name("MODE")
                .possible_values(&[
                    "gossip",
                    "tvu",
                    "tvu_forwards",
                    "tpu",
                    "tpu_forwards",
                    "repair",
                    "serve_repair",
                    "rpc",
                ])
                .help("Interface to DoS"),
        )
        .arg(
            Arg::with_name("data_size")
                .long("data-size")
                .takes_value(true)
                .value_name("BYTES")
                .help("Size of packet to DoS with"),
        )
        .arg(
            Arg::with_name("data_type")
                .long("data-type")
                .takes_value(true)
                .value_name("TYPE")
                .possible_values(&[
                    "repair_highest",
                    "repair_shred",
                    "repair_orphan",
                    "random",
                    "get_account_info",
                    "get_program_accounts",
                    "transaction",
                ])
                .help("Type of data to send"),
        )
        .arg(
            Arg::with_name("data_input")
                .long("data-input")
                .takes_value(true)
                .value_name("TYPE")
                .help("Data to send"),
        )
        .arg(
            Arg::with_name("skip_gossip")
                .long("skip-gossip")
                .help("Just use entrypoint address directly"),
        )
        .arg(
            Arg::with_name("allow_private_addr")
                .long("allow-private-addr")
                .takes_value(false)
                .help("Allow contacting private ip addresses")
                .hidden(true),
        )
        .arg(
            Arg::with_name("num_sign")
                .long("number-of-signatures")
                .takes_value(true)
                .help("Number of signatures in transaction"),
        )
        .arg(
            Arg::with_name("valid_blockhash")
                .long("generate-valid-blockhash")
                .takes_value(false)
                .help("Generate a valid blockhash for transaction")
                .hidden(true),
        )
        .arg(
            Arg::with_name("valid_sign")
                .long("generate-valid-signatures")
                .takes_value(false)
                .help("Generate valid signature(s) for transaction")
                .hidden(true),
        )
        .arg(
            Arg::with_name("unique_trans")
                .long("generate-unique-transactions")
                .takes_value(false)
                .help("Generate unique transaction")
                .hidden(true),
        )
        .arg(
            Arg::with_name("payer")
                .long("payer")
                .takes_value(false)
                .value_name("FILE")
                .help("Payer's keypair to fund transactions")
                .hidden(true),
        )
        .get_matches();

    let mut entrypoint_addr = SocketAddr::from(([127, 0, 0, 1], 8001));
    if let Some(addr) = matches.value_of("entrypoint") {
        entrypoint_addr = solana_net_utils::parse_host_port(addr).unwrap_or_else(|e| {
            eprintln!("failed to parse entrypoint address: {}", e);
            exit(1)
        });
    }
    let data_size = value_t!(matches, "data_size", usize).unwrap_or(128);
    let skip_gossip = matches.is_present("skip_gossip");

    let mode = value_t_or_exit!(matches, "mode", String);
    let data_type = value_t_or_exit!(matches, "data_type", String);
    let data_input = value_t!(matches, "data_input", String).ok();

    let transaction_params = match data_type.as_str() {
        "transaction" => Some(TransactionParams {
            unique_transactions: matches.is_present("unique_trans"),
            num_sign: value_t!(matches, "num_sign", usize).unwrap_or(2),
            valid_block_hash: matches.is_present("valid_blockhash"),
            valid_signatures: matches.is_present("valid_sign"),
            with_payer: matches.is_present("payer"),
        }),
        _ => None,
    };

    let mut nodes = vec![];
    if !skip_gossip {
        info!("Finding cluster entry: {:?}", entrypoint_addr);
        let socket_addr_space = SocketAddrSpace::new(matches.is_present("allow_private_addr"));
        let (gossip_nodes, _validators) = discover(
            None, // keypair
            Some(&entrypoint_addr),
            None,                    // num_nodes
            Duration::from_secs(60), // timeout
            None,                    // find_node_by_pubkey
            Some(&entrypoint_addr),  // find_node_by_gossip_addr
            None,                    // my_gossip_addr
            0,                       // my_shred_version
            socket_addr_space,
        )
        .unwrap_or_else(|err| {
            eprintln!("Failed to discover {} node: {:?}", entrypoint_addr, err);
            exit(1);
        });
        nodes = gossip_nodes;
    }

    let payer = if transaction_params.is_some() && transaction_params.as_ref().unwrap().with_payer {
        let keypair_file_name = value_t_or_exit!(matches, "payer", String);
        read_keypair_file(&keypair_file_name)
            .unwrap_or_else(|_| panic!("bad keypair {:?}", keypair_file_name))
    } else {
        Keypair::new()
    };

    info!("done found {} nodes", nodes.len());
    run_dos(
        &payer,
        &nodes,
        0,
        entrypoint_addr,
        data_type,
        data_size,
        mode,
        data_input,
        transaction_params,
    );
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

        let payer = Keypair::new();

        run_dos(
            &payer,
            &nodes,
            1,
            entrypoint_addr,
            "random".to_string(),
            10,
            "tvu".to_string(),
            None,
            None,
        );

        run_dos(
            &payer,
            &nodes,
            1,
            entrypoint_addr,
            "repair_highest".to_string(),
            10,
            "repair".to_string(),
            None,
            None,
        );

        run_dos(
            &payer,
            &nodes,
            1,
            entrypoint_addr,
            "repair_shred".to_string(),
            10,
            "serve_repair".to_string(),
            None,
            None,
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

        let tp = Some(TransactionParams {
            unique_transactions: true,
            num_sign: 2,
            valid_block_hash: true, // use valid blockhash or random
            valid_signatures: true, // use valid signatures or not
            with_payer: true,
        });

        run_dos(
            &cluster.funding_keypair,
            &[node],
            10_000_000,
            cluster.entry_point_info.gossip,
            "transaction".to_string(),
            1000,
            "tpu".to_string(),
            None,
            tp,
        );
    }
}
