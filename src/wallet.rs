use bincode::serialize;
use bs58;
use budget_program;
use budget_transaction::BudgetTransaction;
use chrono::prelude::*;
use clap::ArgMatches;
use elf;
use fullnode::Config;
use loader_transaction::LoaderTransaction;
use ring::rand::SystemRandom;
use ring::signature::Ed25519KeyPair;
use rpc::RpcSignatureStatus;
use rpc_request::{get_rpc_request_str, RpcClient, RpcRequest};
use serde_json;
use solana_drone::drone::{request_airdrop_transaction, DRONE_PORT};
use solana_sdk::bpf_loader;
use solana_sdk::hash::Hash;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Keypair, KeypairUtil, Signature};
use solana_sdk::transaction::Transaction;
use std::fs::{self, File};
use std::io::Write;
use std::net::{Ipv4Addr, SocketAddr};
use std::path::Path;
use std::str::FromStr;
use std::thread::sleep;
use std::time::Duration;
use std::{error, fmt, mem};
use system_transaction::SystemTransaction;
use thin_client::poll_gossip_for_leader;

const PLATFORM_SECTION_C: &str = ".text.entrypoint";
const USERDATA_CHUNK_SIZE: usize = 256;

#[derive(Debug, PartialEq)]
pub enum WalletCommand {
    Address,
    AirDrop(u64),
    Balance,
    Cancel(Pubkey),
    Confirm(Signature),
    Deploy(String),
    GetTransactionCount,
    // Pay(tokens, to, timestamp, timestamp_pubkey, witness(es), cancelable)
    Pay(
        u64,
        Pubkey,
        Option<DateTime<Utc>>,
        Option<Pubkey>,
        Option<Vec<Pubkey>>,
        Option<Pubkey>,
    ),
    // TimeElapsed(to, process_id, timestamp)
    TimeElapsed(Pubkey, Pubkey, DateTime<Utc>),
    // Witness(to, process_id)
    Witness(Pubkey, Pubkey),
}

#[derive(Debug, Clone)]
pub enum WalletError {
    CommandNotRecognized(String),
    BadParameter(String),
    DynamicProgramError(String),
    RpcRequestError(String),
}

impl fmt::Display for WalletError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "invalid")
    }
}

impl error::Error for WalletError {
    fn description(&self) -> &str {
        "invalid"
    }

    fn cause(&self) -> Option<&error::Error> {
        // Generic error, underlying cause isn't tracked.
        None
    }
}

pub struct WalletConfig {
    pub id: Keypair,
    pub command: WalletCommand,
    pub network: SocketAddr,
    pub timeout: Option<u64>,
    pub proxy: Option<String>,
    pub drone_port: Option<u16>,
}

impl Default for WalletConfig {
    fn default() -> WalletConfig {
        let default_addr = socketaddr!(0, 8000);
        WalletConfig {
            id: Keypair::new(),
            command: WalletCommand::Balance,
            network: default_addr,
            timeout: None,
            proxy: None,
            drone_port: None,
        }
    }
}

impl WalletConfig {
    pub fn drone_addr(&self, tpu_addr: SocketAddr) -> SocketAddr {
        let mut drone_addr = tpu_addr;
        drone_addr.set_port(self.drone_port.unwrap_or(DRONE_PORT));
        drone_addr
    }

    pub fn rpc_addr(&self, rpc_addr: SocketAddr) -> String {
        let rpc_addr_str = get_rpc_request_str(rpc_addr);
        self.proxy.clone().unwrap_or(rpc_addr_str)
    }
}

pub fn parse_command(
    pubkey: Pubkey,
    matches: &ArgMatches,
) -> Result<WalletCommand, Box<error::Error>> {
    let response = match matches.subcommand() {
        ("address", Some(_address_matches)) => Ok(WalletCommand::Address),
        ("airdrop", Some(airdrop_matches)) => {
            let tokens = airdrop_matches.value_of("tokens").unwrap().parse()?;
            Ok(WalletCommand::AirDrop(tokens))
        }
        ("balance", Some(_balance_matches)) => Ok(WalletCommand::Balance),
        ("cancel", Some(cancel_matches)) => {
            let pubkey_vec = bs58::decode(cancel_matches.value_of("process-id").unwrap())
                .into_vec()
                .expect("base58-encoded public key");

            if pubkey_vec.len() != mem::size_of::<Pubkey>() {
                eprintln!("{}", cancel_matches.usage());
                Err(WalletError::BadParameter("Invalid public key".to_string()))?;
            }
            let process_id = Pubkey::new(&pubkey_vec);
            Ok(WalletCommand::Cancel(process_id))
        }
        ("confirm", Some(confirm_matches)) => {
            let signatures = bs58::decode(confirm_matches.value_of("signature").unwrap())
                .into_vec()
                .expect("base58-encoded signature");

            if signatures.len() == mem::size_of::<Signature>() {
                let signature = Signature::new(&signatures);
                Ok(WalletCommand::Confirm(signature))
            } else {
                eprintln!("{}", confirm_matches.usage());
                Err(WalletError::BadParameter("Invalid signature".to_string()))
            }
        }
        ("deploy", Some(deploy_matches)) => Ok(WalletCommand::Deploy(
            deploy_matches
                .value_of("program-location")
                .unwrap()
                .to_string(),
        )),
        ("get-transaction-count", Some(_matches)) => Ok(WalletCommand::GetTransactionCount),
        ("pay", Some(pay_matches)) => {
            let tokens = pay_matches.value_of("tokens").unwrap().parse()?;
            let to = if pay_matches.is_present("to") {
                let pubkey_vec = bs58::decode(pay_matches.value_of("to").unwrap())
                    .into_vec()
                    .expect("base58-encoded public key");

                if pubkey_vec.len() != mem::size_of::<Pubkey>() {
                    eprintln!("{}", pay_matches.usage());
                    Err(WalletError::BadParameter(
                        "Invalid to public key".to_string(),
                    ))?;
                }
                Pubkey::new(&pubkey_vec)
            } else {
                pubkey
            };
            let timestamp = if pay_matches.is_present("timestamp") {
                // Parse input for serde_json
                let date_string = if !pay_matches.value_of("timestamp").unwrap().contains('Z') {
                    format!("\"{}Z\"", pay_matches.value_of("timestamp").unwrap())
                } else {
                    format!("\"{}\"", pay_matches.value_of("timestamp").unwrap())
                };
                Some(serde_json::from_str(&date_string)?)
            } else {
                None
            };
            let timestamp_pubkey = if pay_matches.is_present("timestamp-pubkey") {
                let pubkey_vec = bs58::decode(pay_matches.value_of("timestamp-pubkey").unwrap())
                    .into_vec()
                    .expect("base58-encoded public key");

                if pubkey_vec.len() != mem::size_of::<Pubkey>() {
                    eprintln!("{}", pay_matches.usage());
                    Err(WalletError::BadParameter(
                        "Invalid timestamp public key".to_string(),
                    ))?;
                }
                Some(Pubkey::new(&pubkey_vec))
            } else {
                None
            };
            let witness_vec = if pay_matches.is_present("witness") {
                let witnesses = pay_matches.values_of("witness").unwrap();
                let mut collection = Vec::new();
                for witness in witnesses {
                    let pubkey_vec = bs58::decode(witness)
                        .into_vec()
                        .expect("base58-encoded public key");

                    if pubkey_vec.len() != mem::size_of::<Pubkey>() {
                        eprintln!("{}", pay_matches.usage());
                        Err(WalletError::BadParameter(
                            "Invalid witness public key".to_string(),
                        ))?;
                    }
                    collection.push(Pubkey::new(&pubkey_vec));
                }
                Some(collection)
            } else {
                None
            };
            let cancelable = if pay_matches.is_present("cancelable") {
                Some(pubkey)
            } else {
                None
            };

            Ok(WalletCommand::Pay(
                tokens,
                to,
                timestamp,
                timestamp_pubkey,
                witness_vec,
                cancelable,
            ))
        }
        ("send-signature", Some(sig_matches)) => {
            let pubkey_vec = bs58::decode(sig_matches.value_of("to").unwrap())
                .into_vec()
                .expect("base58-encoded public key");

            if pubkey_vec.len() != mem::size_of::<Pubkey>() {
                eprintln!("{}", sig_matches.usage());
                Err(WalletError::BadParameter("Invalid public key".to_string()))?;
            }
            let to = Pubkey::new(&pubkey_vec);

            let pubkey_vec = bs58::decode(sig_matches.value_of("process-id").unwrap())
                .into_vec()
                .expect("base58-encoded public key");

            if pubkey_vec.len() != mem::size_of::<Pubkey>() {
                eprintln!("{}", sig_matches.usage());
                Err(WalletError::BadParameter("Invalid public key".to_string()))?;
            }
            let process_id = Pubkey::new(&pubkey_vec);
            Ok(WalletCommand::Witness(to, process_id))
        }
        ("send-timestamp", Some(timestamp_matches)) => {
            let pubkey_vec = bs58::decode(timestamp_matches.value_of("to").unwrap())
                .into_vec()
                .expect("base58-encoded public key");

            if pubkey_vec.len() != mem::size_of::<Pubkey>() {
                eprintln!("{}", timestamp_matches.usage());
                Err(WalletError::BadParameter("Invalid public key".to_string()))?;
            }
            let to = Pubkey::new(&pubkey_vec);

            let pubkey_vec = bs58::decode(timestamp_matches.value_of("process-id").unwrap())
                .into_vec()
                .expect("base58-encoded public key");

            if pubkey_vec.len() != mem::size_of::<Pubkey>() {
                eprintln!("{}", timestamp_matches.usage());
                Err(WalletError::BadParameter("Invalid public key".to_string()))?;
            }
            let process_id = Pubkey::new(&pubkey_vec);
            let dt = if timestamp_matches.is_present("datetime") {
                // Parse input for serde_json
                let date_string = if !timestamp_matches
                    .value_of("datetime")
                    .unwrap()
                    .contains('Z')
                {
                    format!("\"{}Z\"", timestamp_matches.value_of("datetime").unwrap())
                } else {
                    format!("\"{}\"", timestamp_matches.value_of("datetime").unwrap())
                };
                serde_json::from_str(&date_string)?
            } else {
                Utc::now()
            };
            Ok(WalletCommand::TimeElapsed(to, process_id, dt))
        }
        ("", None) => {
            eprintln!("{}", matches.usage());
            Err(WalletError::CommandNotRecognized(
                "no subcommand given".to_string(),
            ))
        }
        _ => unreachable!(),
    }?;
    Ok(response)
}

pub fn process_command(config: &WalletConfig) -> Result<String, Box<error::Error>> {
    if let WalletCommand::Address = config.command {
        // Get address of this client
        return Ok(format!("{}", config.id.pubkey()));
    }

    let leader = poll_gossip_for_leader(config.network, config.timeout)?;
    let tpu_addr = leader.tpu;
    let drone_addr = config.drone_addr(tpu_addr);
    let rpc_addr = config.rpc_addr(leader.rpc);
    let rpc_client = RpcClient::new(rpc_addr);

    match config.command {
        // Get address of this client
        WalletCommand::Address => unreachable!(),
        // Request an airdrop from Solana Drone;
        WalletCommand::AirDrop(tokens) => {
            println!(
                "Requesting airdrop of {:?} tokens from {}",
                tokens, drone_addr
            );
            let params = json!([format!("{}", config.id.pubkey())]);
            let previous_balance = match RpcRequest::GetBalance
                .make_rpc_request(&rpc_client, 1, Some(params))?
                .as_u64()
            {
                Some(tokens) => tokens,
                None => Err(WalletError::RpcRequestError(
                    "Received result of an unexpected type".to_string(),
                ))?,
            };

            let last_id = get_last_id(&rpc_client)?;
            let transaction =
                request_airdrop_transaction(&drone_addr, &config.id.pubkey(), tokens, last_id)?;
            send_and_confirm_tx(&rpc_client, &transaction)?;

            let params = json!([format!("{}", config.id.pubkey())]);
            let current_balance = RpcRequest::GetBalance
                .make_rpc_request(&rpc_client, 1, Some(params))?
                .as_u64()
                .unwrap_or(previous_balance);

            if current_balance - previous_balance < tokens {
                Err("Airdrop failed!")?;
            }
            Ok(format!("Your balance is: {:?}", current_balance))
        }
        // Check client balance
        WalletCommand::Balance => {
            println!("Balance requested...");
            let params = json!([format!("{}", config.id.pubkey())]);
            let balance = RpcRequest::GetBalance
                .make_rpc_request(&rpc_client, 1, Some(params))?
                .as_u64();
            match balance {
                Some(0) => Ok("No account found! Request an airdrop to get started.".to_string()),
                Some(tokens) => Ok(format!("Your balance is: {:?}", tokens)),
                None => Err(WalletError::RpcRequestError(
                    "Received result of an unexpected type".to_string(),
                ))?,
            }
        }
        // Cancel a contract by contract Pubkey
        WalletCommand::Cancel(pubkey) => {
            let last_id = get_last_id(&rpc_client)?;
            let tx =
                Transaction::budget_new_signature(&config.id, pubkey, config.id.pubkey(), last_id);
            let signature_str = send_tx(&rpc_client, &tx)?;
            Ok(signature_str.to_string())
        }
        // Confirm the last client transaction by signature
        WalletCommand::Confirm(signature) => {
            let params = json!([format!("{}", signature)]);
            let confirmation = RpcRequest::ConfirmTransaction
                .make_rpc_request(&rpc_client, 1, Some(params))?
                .as_bool();
            match confirmation {
                Some(b) => {
                    if b {
                        Ok("Confirmed".to_string())
                    } else {
                        Ok("Not found".to_string())
                    }
                }
                None => Err(WalletError::RpcRequestError(
                    "Received result of an unexpected type".to_string(),
                ))?,
            }
        }
        // Deploy a custom program to the chain
        WalletCommand::Deploy(ref program_location) => {
            let params = json!([format!("{}", config.id.pubkey())]);
            let balance = RpcRequest::GetBalance
                .make_rpc_request(&rpc_client, 1, Some(params))?
                .as_u64();
            if let Some(tokens) = balance {
                if tokens < 1 {
                    Err(WalletError::DynamicProgramError(
                        "Insufficient funds".to_string(),
                    ))?
                }
            }

            let last_id = get_last_id(&rpc_client)?;
            let program = Keypair::new();
            let program_userdata = elf::File::open_path(program_location)
                .map_err(|_| {
                    WalletError::DynamicProgramError("Could not parse program file".to_string())
                })?.get_section(PLATFORM_SECTION_C)
                .ok_or_else(|| {
                    WalletError::DynamicProgramError(
                        "Could not find entrypoint in program file".to_string(),
                    )
                })?.data
                .clone();

            let tx = Transaction::system_create(
                &config.id,
                program.pubkey(),
                last_id,
                1,
                program_userdata.len() as u64,
                bpf_loader::id(),
                0,
            );
            send_and_confirm_tx(&rpc_client, &tx).map_err(|_| {
                WalletError::DynamicProgramError("Program allocate space failed".to_string())
            })?;

            let mut offset = 0;
            for chunk in program_userdata.chunks(USERDATA_CHUNK_SIZE) {
                let tx = Transaction::loader_write(
                    &program,
                    bpf_loader::id(),
                    offset,
                    chunk.to_vec(),
                    last_id,
                    0,
                );
                send_and_confirm_tx(&rpc_client, &tx).map_err(|_| {
                    WalletError::DynamicProgramError(format!(
                        "Program write failed at offset {:?}",
                        offset
                    ))
                })?;
                offset += USERDATA_CHUNK_SIZE as u32;
            }

            let last_id = get_last_id(&rpc_client)?;
            let tx = Transaction::loader_finalize(&program, bpf_loader::id(), last_id, 0);
            send_and_confirm_tx(&rpc_client, &tx).map_err(|_| {
                WalletError::DynamicProgramError("Program finalize transaction failed".to_string())
            })?;

            let tx = Transaction::system_spawn(&program, last_id, 0);
            send_and_confirm_tx(&rpc_client, &tx).map_err(|_| {
                WalletError::DynamicProgramError("Program spawn failed".to_string())
            })?;

            Ok(json!({
                "programId": format!("{}", program.pubkey()),
            }).to_string())
        }
        WalletCommand::GetTransactionCount => {
            let transaction_count = RpcRequest::GetTransactionCount
                .make_rpc_request(&rpc_client, 1, None)?
                .as_u64();
            match transaction_count {
                Some(count) => Ok(count.to_string()),
                None => Err(WalletError::RpcRequestError(
                    "Received result of an unexpected type".to_string(),
                ))?,
            }
        }
        // If client has positive balance, pay tokens to another address
        WalletCommand::Pay(tokens, to, timestamp, timestamp_pubkey, ref witnesses, cancelable) => {
            let last_id = get_last_id(&rpc_client)?;

            if timestamp == None && *witnesses == None {
                let tx = Transaction::system_new(&config.id, to, tokens, last_id);
                let signature_str = send_tx(&rpc_client, &tx)?;
                Ok(signature_str.to_string())
            } else if *witnesses == None {
                let dt = timestamp.unwrap();
                let dt_pubkey = match timestamp_pubkey {
                    Some(pubkey) => pubkey,
                    None => config.id.pubkey(),
                };

                let contract_funds = Keypair::new();
                let contract_state = Keypair::new();
                let budget_program_id = budget_program::id();

                // Create account for contract funds
                let tx = Transaction::system_create(
                    &config.id,
                    contract_funds.pubkey(),
                    last_id,
                    tokens,
                    0,
                    budget_program_id,
                    0,
                );
                let _signature_str = send_tx(&rpc_client, &tx)?;

                // Create account for contract state
                let tx = Transaction::system_create(
                    &config.id,
                    contract_state.pubkey(),
                    last_id,
                    1,
                    196,
                    budget_program_id,
                    0,
                );
                let _signature_str = send_tx(&rpc_client, &tx)?;

                // Initializing contract
                let tx = Transaction::budget_new_on_date(
                    &contract_funds,
                    to,
                    contract_state.pubkey(),
                    dt,
                    dt_pubkey,
                    cancelable,
                    tokens,
                    last_id,
                );
                let signature_str = send_tx(&rpc_client, &tx)?;

                Ok(json!({
                    "signature": signature_str,
                    "processId": format!("{}", contract_state.pubkey()),
                }).to_string())
            } else if timestamp == None {
                let last_id = get_last_id(&rpc_client)?;

                let witness = if let Some(ref witness_vec) = *witnesses {
                    witness_vec[0]
                } else {
                    Err(WalletError::BadParameter(
                        "Could not parse required signature pubkey(s)".to_string(),
                    ))?
                };

                let contract_funds = Keypair::new();
                let contract_state = Keypair::new();
                let budget_program_id = budget_program::id();

                // Create account for contract funds
                let tx = Transaction::system_create(
                    &config.id,
                    contract_funds.pubkey(),
                    last_id,
                    tokens,
                    0,
                    budget_program_id,
                    0,
                );
                let _signature_str = send_tx(&rpc_client, &tx)?;

                // Create account for contract state
                let tx = Transaction::system_create(
                    &config.id,
                    contract_state.pubkey(),
                    last_id,
                    1,
                    196,
                    budget_program_id,
                    0,
                );
                let _signature_str = send_tx(&rpc_client, &tx)?;

                // Initializing contract
                let tx = Transaction::budget_new_when_signed(
                    &contract_funds,
                    to,
                    contract_state.pubkey(),
                    witness,
                    cancelable,
                    tokens,
                    last_id,
                );
                let signature_str = send_tx(&rpc_client, &tx)?;

                Ok(json!({
                    "signature": signature_str,
                    "processId": format!("{}", contract_state.pubkey()),
                }).to_string())
            } else {
                Ok("Combo transactions not yet handled".to_string())
            }
        }
        // Apply time elapsed to contract
        WalletCommand::TimeElapsed(to, pubkey, dt) => {
            let params = json!(format!("{}", config.id.pubkey()));
            let balance = RpcRequest::GetBalance
                .make_rpc_request(&rpc_client, 1, Some(params))?
                .as_u64();

            if let Some(0) = balance {
                let params = json!([format!("{}", config.id.pubkey()), 1]);
                RpcRequest::RequestAirdrop
                    .make_rpc_request(&rpc_client, 1, Some(params))
                    .unwrap();
            }

            let last_id = get_last_id(&rpc_client)?;

            let tx = Transaction::budget_new_timestamp(&config.id, pubkey, to, dt, last_id);
            let signature_str = send_tx(&rpc_client, &tx)?;

            Ok(signature_str.to_string())
        }
        // Apply witness signature to contract
        WalletCommand::Witness(to, pubkey) => {
            let params = json!([format!("{}", config.id.pubkey())]);
            let balance = RpcRequest::GetBalance
                .make_rpc_request(&rpc_client, 1, Some(params))?
                .as_u64();

            if let Some(0) = balance {
                let params = json!([format!("{}", config.id.pubkey()), 1]);
                RpcRequest::RequestAirdrop
                    .make_rpc_request(&rpc_client, 1, Some(params))
                    .unwrap();
            }

            let last_id = get_last_id(&rpc_client)?;
            let tx = Transaction::budget_new_signature(&config.id, pubkey, to, last_id);
            let signature_str = send_tx(&rpc_client, &tx)?;

            Ok(signature_str.to_string())
        }
    }
}

pub fn read_leader(path: &str) -> Result<Config, WalletError> {
    let file = File::open(path.to_string()).or_else(|err| {
        Err(WalletError::BadParameter(format!(
            "{}: Unable to open leader file: {}",
            err, path
        )))
    })?;

    serde_json::from_reader(file).or_else(|err| {
        Err(WalletError::BadParameter(format!(
            "{}: Failed to parse leader file: {}",
            err, path
        )))
    })
}

pub fn gen_keypair_file(outfile: String) -> Result<String, Box<error::Error>> {
    let rnd = SystemRandom::new();
    let pkcs8_bytes = Ed25519KeyPair::generate_pkcs8(&rnd)?;
    let serialized = serde_json::to_string(&pkcs8_bytes.to_vec())?;

    if outfile != "-" {
        if let Some(outdir) = Path::new(&outfile).parent() {
            fs::create_dir_all(outdir)?;
        }
        let mut f = File::create(outfile)?;
        f.write_all(&serialized.clone().into_bytes())?;
    }
    Ok(serialized)
}

fn get_last_id(rpc_client: &RpcClient) -> Result<Hash, Box<error::Error>> {
    let result = RpcRequest::GetLastId.make_rpc_request(rpc_client, 1, None)?;
    if result.as_str().is_none() {
        Err(WalletError::RpcRequestError(
            "Received bad last_id".to_string(),
        ))?
    }
    let last_id_str = result.as_str().unwrap();
    let last_id_vec = bs58::decode(last_id_str)
        .into_vec()
        .map_err(|_| WalletError::RpcRequestError("Received bad last_id".to_string()))?;
    Ok(Hash::new(&last_id_vec))
}

fn send_tx(rpc_client: &RpcClient, tx: &Transaction) -> Result<String, Box<error::Error>> {
    let serialized = serialize(tx).unwrap();
    let params = json!([serialized]);
    let signature = RpcRequest::SendTransaction.make_rpc_request(rpc_client, 2, Some(params))?;
    if signature.as_str().is_none() {
        Err(WalletError::RpcRequestError(
            "Received result of an unexpected type".to_string(),
        ))?
    }
    Ok(signature.as_str().unwrap().to_string())
}

fn confirm_tx(
    rpc_client: &RpcClient,
    signature: &str,
) -> Result<RpcSignatureStatus, Box<error::Error>> {
    let params = json!([signature.to_string()]);
    let signature_status =
        RpcRequest::GetSignatureStatus.make_rpc_request(rpc_client, 1, Some(params))?;
    if let Some(status) = signature_status.as_str() {
        let rpc_status = RpcSignatureStatus::from_str(status).map_err(|_| {
            WalletError::RpcRequestError("Unable to parse signature status".to_string())
        })?;
        Ok(rpc_status)
    } else {
        Err(WalletError::RpcRequestError(
            "Received result of an unexpected type".to_string(),
        ))?
    }
}

fn send_and_confirm_tx(rpc_client: &RpcClient, tx: &Transaction) -> Result<(), Box<error::Error>> {
    let mut send_retries = 3;
    while send_retries > 0 {
        let mut status_retries = 4;
        let signature_str = send_tx(rpc_client, tx)?;
        let status = loop {
            let status = confirm_tx(rpc_client, &signature_str)?;
            if status == RpcSignatureStatus::SignatureNotFound {
                status_retries -= 1;
                if status_retries == 0 {
                    break status;
                }
            } else {
                break status;
            }
            sleep(Duration::from_secs(1));
        };
        match status {
            RpcSignatureStatus::AccountInUse => {
                send_retries -= 1;
            }
            RpcSignatureStatus::Confirmed => {
                return Ok(());
            }
            _ => {
                return Err(WalletError::RpcRequestError(format!(
                    "Transaction {:?} failed: {:?}",
                    signature_str, status
                )))?;
            }
        }
    }
    Err(WalletError::RpcRequestError(format!(
        "AccountInUse after 3 retries: {:?}",
        tx.account_keys[0]
    )))?
}

#[cfg(test)]
mod tests {
    use super::*;
    use bank::Bank;
    use clap::{App, Arg, SubCommand};
    use cluster_info::Node;
    use fullnode::Fullnode;
    use leader_scheduler::LeaderScheduler;
    use ledger::create_tmp_genesis;
    use serde_json::Value;
    use solana_drone::drone::run_local_drone;
    use solana_sdk::signature::{read_keypair, read_pkcs8, Keypair, KeypairUtil};
    use std::fs::remove_dir_all;
    use std::sync::mpsc::channel;
    use std::sync::{Arc, RwLock};
    use std::thread::sleep;
    use std::time::Duration;

    #[test]
    fn test_wallet_parse_command() {
        let test_commands = App::new("test")
            .subcommand(SubCommand::with_name("address").about("Get your public key"))
            .subcommand(
                SubCommand::with_name("airdrop")
                    .about("Request a batch of tokens")
                    .arg(
                        Arg::with_name("tokens")
                            .index(1)
                            .value_name("NUM")
                            .takes_value(true)
                            .required(true)
                            .help("The number of tokens to request"),
                    ),
            ).subcommand(SubCommand::with_name("balance").about("Get your balance"))
            .subcommand(
                SubCommand::with_name("cancel")
                    .about("Cancel a transfer")
                    .arg(
                        Arg::with_name("process-id")
                            .index(1)
                            .value_name("PROCESS_ID")
                            .takes_value(true)
                            .required(true)
                            .help("The process id of the transfer to cancel"),
                    ),
            ).subcommand(
                SubCommand::with_name("confirm")
                    .about("Confirm transaction by signature")
                    .arg(
                        Arg::with_name("signature")
                            .index(1)
                            .value_name("SIGNATURE")
                            .takes_value(true)
                            .required(true)
                            .help("The transaction signature to confirm"),
                    ),
            ).subcommand(
                SubCommand::with_name("deploy")
                    .about("Deploy a program")
                    .arg(
                        Arg::with_name("program-location")
                            .index(1)
                            .value_name("PATH")
                            .takes_value(true)
                            .required(true)
                            .help("/path/to/program.o"),
                    ), // TODO: Add "loader" argument; current default is bpf_loader
            ).subcommand(
                SubCommand::with_name("get-transaction-count")
                    .about("Get current transaction count"),
            ).subcommand(
                SubCommand::with_name("pay")
                    .about("Send a payment")
                    .arg(
                        Arg::with_name("to")
                            .index(1)
                            .value_name("PUBKEY")
                            .takes_value(true)
                            .required(true)
                            .help("The pubkey of recipient"),
                    ).arg(
                        Arg::with_name("tokens")
                            .index(2)
                            .value_name("NUM")
                            .takes_value(true)
                            .required(true)
                            .help("The number of tokens to send"),
                    ).arg(
                        Arg::with_name("timestamp")
                            .long("after")
                            .value_name("DATETIME")
                            .takes_value(true)
                            .help("A timestamp after which transaction will execute"),
                    ).arg(
                        Arg::with_name("timestamp-pubkey")
                            .long("require-timestamp-from")
                            .value_name("PUBKEY")
                            .takes_value(true)
                            .requires("timestamp")
                            .help("Require timestamp from this third party"),
                    ).arg(
                        Arg::with_name("witness")
                            .long("require-signature-from")
                            .value_name("PUBKEY")
                            .takes_value(true)
                            .multiple(true)
                            .use_delimiter(true)
                            .help("Any third party signatures required to unlock the tokens"),
                    ).arg(
                        Arg::with_name("cancelable")
                            .long("cancelable")
                            .takes_value(false),
                    ),
            ).subcommand(
                SubCommand::with_name("send-signature")
                    .about("Send a signature to authorize a transfer")
                    .arg(
                        Arg::with_name("to")
                            .index(1)
                            .value_name("PUBKEY")
                            .takes_value(true)
                            .required(true)
                            .help("The pubkey of recipient"),
                    ).arg(
                        Arg::with_name("process-id")
                            .index(2)
                            .value_name("PROCESS_ID")
                            .takes_value(true)
                            .required(true)
                            .help("The process id of the transfer to authorize"),
                    ),
            ).subcommand(
                SubCommand::with_name("send-timestamp")
                    .about("Send a timestamp to unlock a transfer")
                    .arg(
                        Arg::with_name("to")
                            .index(1)
                            .value_name("PUBKEY")
                            .takes_value(true)
                            .required(true)
                            .help("The pubkey of recipient"),
                    ).arg(
                        Arg::with_name("process-id")
                            .index(2)
                            .value_name("PROCESS_ID")
                            .takes_value(true)
                            .required(true)
                            .help("The process id of the transfer to unlock"),
                    ).arg(
                        Arg::with_name("datetime")
                            .long("date")
                            .value_name("DATETIME")
                            .takes_value(true)
                            .help("Optional arbitrary timestamp to apply"),
                    ),
            );
        let pubkey = Keypair::new().pubkey();
        let pubkey_string = format!("{}", pubkey);
        let witness0 = Keypair::new().pubkey();
        let witness0_string = format!("{}", witness0);
        let witness1 = Keypair::new().pubkey();
        let witness1_string = format!("{}", witness1);
        let dt = Utc.ymd(2018, 9, 19).and_hms(17, 30, 59);

        // Test Airdrop Subcommand
        let test_airdrop = test_commands
            .clone()
            .get_matches_from(vec!["test", "airdrop", "50"]);
        assert_eq!(
            parse_command(pubkey, &test_airdrop).unwrap(),
            WalletCommand::AirDrop(50)
        );
        let test_bad_airdrop = test_commands
            .clone()
            .get_matches_from(vec!["test", "airdrop", "notint"]);
        assert!(parse_command(pubkey, &test_bad_airdrop).is_err());

        // Test Cancel Subcommand
        let test_cancel =
            test_commands
                .clone()
                .get_matches_from(vec!["test", "cancel", &pubkey_string]);
        assert_eq!(
            parse_command(pubkey, &test_cancel).unwrap(),
            WalletCommand::Cancel(pubkey)
        );

        // Test Confirm Subcommand
        let signature = Signature::new(&vec![1; 64]);
        let signature_string = format!("{:?}", signature);
        let test_confirm =
            test_commands
                .clone()
                .get_matches_from(vec!["test", "confirm", &signature_string]);
        assert_eq!(
            parse_command(pubkey, &test_confirm).unwrap(),
            WalletCommand::Confirm(signature)
        );
        let test_bad_signature = test_commands
            .clone()
            .get_matches_from(vec!["test", "confirm", "deadbeef"]);
        assert!(parse_command(pubkey, &test_bad_signature).is_err());

        // Test Deploy Subcommand
        let test_deploy =
            test_commands
                .clone()
                .get_matches_from(vec!["test", "deploy", "/Users/test/program.o"]);
        assert_eq!(
            parse_command(pubkey, &test_deploy).unwrap(),
            WalletCommand::Deploy("/Users/test/program.o".to_string())
        );

        // Test Simple Pay Subcommand
        let test_pay =
            test_commands
                .clone()
                .get_matches_from(vec!["test", "pay", &pubkey_string, "50"]);
        assert_eq!(
            parse_command(pubkey, &test_pay).unwrap(),
            WalletCommand::Pay(50, pubkey, None, None, None, None)
        );
        let test_bad_pubkey = test_commands
            .clone()
            .get_matches_from(vec!["test", "pay", "deadbeef", "50"]);
        assert!(parse_command(pubkey, &test_bad_pubkey).is_err());

        // Test Pay Subcommand w/ Witness
        let test_pay_multiple_witnesses = test_commands.clone().get_matches_from(vec![
            "test",
            "pay",
            &pubkey_string,
            "50",
            "--require-signature-from",
            &witness0_string,
            "--require-signature-from",
            &witness1_string,
        ]);
        assert_eq!(
            parse_command(pubkey, &test_pay_multiple_witnesses).unwrap(),
            WalletCommand::Pay(50, pubkey, None, None, Some(vec![witness0, witness1]), None)
        );
        let test_pay_single_witness = test_commands.clone().get_matches_from(vec![
            "test",
            "pay",
            &pubkey_string,
            "50",
            "--require-signature-from",
            &witness0_string,
        ]);
        assert_eq!(
            parse_command(pubkey, &test_pay_single_witness).unwrap(),
            WalletCommand::Pay(50, pubkey, None, None, Some(vec![witness0]), None)
        );

        // Test Pay Subcommand w/ Timestamp
        let test_pay_timestamp = test_commands.clone().get_matches_from(vec![
            "test",
            "pay",
            &pubkey_string,
            "50",
            "--after",
            "2018-09-19T17:30:59",
            "--require-timestamp-from",
            &witness0_string,
        ]);
        assert_eq!(
            parse_command(pubkey, &test_pay_timestamp).unwrap(),
            WalletCommand::Pay(50, pubkey, Some(dt), Some(witness0), None, None)
        );

        // Test Send-Signature Subcommand
        let test_send_signature = test_commands.clone().get_matches_from(vec![
            "test",
            "send-signature",
            &pubkey_string,
            &pubkey_string,
        ]);
        assert_eq!(
            parse_command(pubkey, &test_send_signature).unwrap(),
            WalletCommand::Witness(pubkey, pubkey)
        );
        let test_pay_multiple_witnesses = test_commands.clone().get_matches_from(vec![
            "test",
            "pay",
            &pubkey_string,
            "50",
            "--after",
            "2018-09-19T17:30:59",
            "--require-signature-from",
            &witness0_string,
            "--require-timestamp-from",
            &witness0_string,
            "--require-signature-from",
            &witness1_string,
        ]);
        assert_eq!(
            parse_command(pubkey, &test_pay_multiple_witnesses).unwrap(),
            WalletCommand::Pay(
                50,
                pubkey,
                Some(dt),
                Some(witness0),
                Some(vec![witness0, witness1]),
                None
            )
        );

        // Test Send-Timestamp Subcommand
        let test_send_timestamp = test_commands.clone().get_matches_from(vec![
            "test",
            "send-timestamp",
            &pubkey_string,
            &pubkey_string,
            "--date",
            "2018-09-19T17:30:59",
        ]);
        assert_eq!(
            parse_command(pubkey, &test_send_timestamp).unwrap(),
            WalletCommand::TimeElapsed(pubkey, pubkey, dt)
        );
        let test_bad_timestamp = test_commands.clone().get_matches_from(vec![
            "test",
            "send-timestamp",
            &pubkey_string,
            &pubkey_string,
            "--date",
            "20180919T17:30:59",
        ]);
        assert!(parse_command(pubkey, &test_bad_timestamp).is_err());
    }
    #[test]
    fn test_wallet_process_command() {
        let bob_pubkey = Keypair::new().pubkey();

        let leader_keypair = Arc::new(Keypair::new());
        let leader = Node::new_localhost_with_pubkey(leader_keypair.pubkey());
        let leader_data = leader.info.clone();
        let (alice, ledger_path) =
            create_tmp_genesis("wallet_process_command", 10_000_000, leader_data.id, 1000);
        let mut bank = Bank::new(&alice);

        let leader_scheduler = Arc::new(RwLock::new(LeaderScheduler::from_bootstrap_leader(
            leader_data.id,
        )));
        bank.leader_scheduler = leader_scheduler;
        let vote_account_keypair = Arc::new(Keypair::new());
        let last_id = bank.last_id();

        let server = Fullnode::new_with_bank(
            leader_keypair,
            vote_account_keypair,
            bank,
            0,
            &last_id,
            leader,
            None,
            &ledger_path,
            false,
            None,
        );
        sleep(Duration::from_millis(900));

        let (sender, receiver) = channel();
        run_local_drone(alice.keypair(), sender);
        let drone_addr = receiver.recv().unwrap();

        let mut config = WalletConfig::default();
        config.network = leader_data.ncp;
        config.drone_port = Some(drone_addr.port());

        let tokens = 50;
        config.command = WalletCommand::AirDrop(tokens);
        assert_eq!(
            process_command(&config).unwrap(),
            format!("Your balance is: {:?}", tokens)
        );

        config.command = WalletCommand::Balance;
        assert_eq!(
            process_command(&config).unwrap(),
            format!("Your balance is: {:?}", tokens)
        );

        config.command = WalletCommand::Pay(10, bob_pubkey, None, None, None, None);
        let sig_response = process_command(&config);
        assert!(sig_response.is_ok());

        let signatures = bs58::decode(sig_response.unwrap())
            .into_vec()
            .expect("base58-encoded signature");
        let signature = Signature::new(&signatures);
        config.command = WalletCommand::Confirm(signature);
        assert_eq!(process_command(&config).unwrap(), "Confirmed");

        config.command = WalletCommand::Balance;
        assert_eq!(
            process_command(&config).unwrap(),
            format!("Your balance is: {:?}", tokens - 10)
        );

        server.close().unwrap();
        remove_dir_all(ledger_path).unwrap();
    }
    #[test]
    fn test_wallet_request_airdrop() {
        let leader_keypair = Arc::new(Keypair::new());
        let leader = Node::new_localhost_with_pubkey(leader_keypair.pubkey());
        let leader_data = leader.info.clone();
        let (alice, ledger_path) =
            create_tmp_genesis("wallet_request_airdrop", 10_000_000, leader_data.id, 1000);
        let mut bank = Bank::new(&alice);

        let leader_scheduler = Arc::new(RwLock::new(LeaderScheduler::from_bootstrap_leader(
            leader_data.id,
        )));
        bank.leader_scheduler = leader_scheduler;
        let vote_account_keypair = Arc::new(Keypair::new());
        let last_id = bank.last_id();
        let entry_height = alice.create_entries().len() as u64;
        let server = Fullnode::new_with_bank(
            leader_keypair,
            vote_account_keypair,
            bank,
            entry_height,
            &last_id,
            leader,
            None,
            &ledger_path,
            false,
            None,
        );
        sleep(Duration::from_millis(900));

        let (sender, receiver) = channel();
        run_local_drone(alice.keypair(), sender);
        let drone_addr = receiver.recv().unwrap();

        let mut bob_config = WalletConfig::default();
        bob_config.network = leader_data.ncp;
        bob_config.drone_port = Some(drone_addr.port());
        bob_config.command = WalletCommand::AirDrop(50);

        let sig_response = process_command(&bob_config);
        assert!(sig_response.is_ok());

        let rpc_client = RpcClient::new_from_socket(leader_data.rpc);

        let params = json!([format!("{}", bob_config.id.pubkey())]);
        let balance = RpcRequest::GetBalance
            .make_rpc_request(&rpc_client, 1, Some(params))
            .unwrap()
            .as_u64()
            .unwrap();
        assert_eq!(balance, 50);

        server.close().unwrap();
        remove_dir_all(ledger_path).unwrap();
    }

    fn tmp_file_path(name: &str) -> String {
        use std::env;
        let out_dir = env::var("OUT_DIR").unwrap_or_else(|_| "target".to_string());
        let keypair = Keypair::new();

        format!("{}/tmp/{}-{}", out_dir, name, keypair.pubkey()).to_string()
    }

    #[test]
    fn test_wallet_gen_keypair_file() {
        let outfile = tmp_file_path("test_gen_keypair_file.json");
        let serialized_keypair = gen_keypair_file(outfile.to_string()).unwrap();
        let keypair_vec: Vec<u8> = serde_json::from_str(&serialized_keypair).unwrap();
        assert!(Path::new(&outfile).exists());
        assert_eq!(keypair_vec, read_pkcs8(&outfile).unwrap());
        assert!(read_keypair(&outfile).is_ok());
        assert_eq!(
            read_keypair(&outfile).unwrap().pubkey().as_ref().len(),
            mem::size_of::<Pubkey>()
        );
        fs::remove_file(&outfile).unwrap();
        assert!(!Path::new(&outfile).exists());
    }
    #[test]
    #[ignore]
    fn test_wallet_timestamp_tx() {
        let bob_pubkey = Keypair::new().pubkey();

        let leader_keypair = Arc::new(Keypair::new());
        let leader = Node::new_localhost_with_pubkey(leader_keypair.pubkey());
        let leader_data = leader.info.clone();
        let (alice, ledger_path) =
            create_tmp_genesis("wallet_timestamp_tx", 10_000_000, leader_data.id, 1000);
        let mut bank = Bank::new(&alice);

        let leader_scheduler = Arc::new(RwLock::new(LeaderScheduler::from_bootstrap_leader(
            leader_data.id,
        )));
        bank.leader_scheduler = leader_scheduler;
        let vote_account_keypair = Arc::new(Keypair::new());
        let last_id = bank.last_id();
        let server = Fullnode::new_with_bank(
            leader_keypair,
            vote_account_keypair,
            bank,
            0,
            &last_id,
            leader,
            None,
            &ledger_path,
            false,
            None,
        );
        sleep(Duration::from_millis(900));

        let (sender, receiver) = channel();
        run_local_drone(alice.keypair(), sender);
        let drone_addr = receiver.recv().unwrap();

        let rpc_client = RpcClient::new_from_socket(leader_data.rpc);

        let mut config_payer = WalletConfig::default();
        config_payer.network = leader_data.ncp;
        config_payer.drone_port = Some(drone_addr.port());

        let mut config_witness = WalletConfig::default();
        config_witness.network = leader_data.ncp;
        config_witness.drone_port = Some(drone_addr.port());

        assert_ne!(config_payer.id.pubkey(), config_witness.id.pubkey());

        let last_id = get_last_id(&rpc_client).unwrap();
        let transaction =
            request_airdrop_transaction(&drone_addr, &config_payer.id.pubkey(), 50, last_id)
                .unwrap();
        send_and_confirm_tx(&rpc_client, &transaction).unwrap();

        // Make transaction (from config_payer to bob_pubkey) requiring timestamp from config_witness
        let date_string = "\"2018-09-19T17:30:59Z\"";
        let dt: DateTime<Utc> = serde_json::from_str(&date_string).unwrap();
        config_payer.command = WalletCommand::Pay(
            10,
            bob_pubkey,
            Some(dt),
            Some(config_witness.id.pubkey()),
            None,
            None,
        );
        let sig_response = process_command(&config_payer);
        assert!(sig_response.is_ok());

        let object: Value = serde_json::from_str(&sig_response.unwrap()).unwrap();
        let process_id_str = object.get("processId").unwrap().as_str().unwrap();
        let process_id_vec = bs58::decode(process_id_str)
            .into_vec()
            .expect("base58-encoded public key");
        let process_id = Pubkey::new(&process_id_vec);

        let params = json!([format!("{}", config_payer.id.pubkey())]);
        let config_payer_balance = RpcRequest::GetBalance
            .make_rpc_request(&rpc_client, 1, Some(params))
            .unwrap()
            .as_u64()
            .unwrap();
        assert_eq!(config_payer_balance, 39);
        let params = json!([format!("{}", process_id)]);
        let contract_balance = RpcRequest::GetBalance
            .make_rpc_request(&rpc_client, 1, Some(params))
            .unwrap()
            .as_u64()
            .unwrap();
        assert_eq!(contract_balance, 11);
        let params = json!([format!("{}", bob_pubkey)]);
        let recipient_balance = RpcRequest::GetBalance
            .make_rpc_request(&rpc_client, 1, Some(params))
            .unwrap()
            .as_u64()
            .unwrap();
        assert_eq!(recipient_balance, 0);

        // Sign transaction by config_witness
        config_witness.command = WalletCommand::TimeElapsed(bob_pubkey, process_id, dt);
        let sig_response = process_command(&config_witness);
        assert!(sig_response.is_ok());

        let params = json!([format!("{}", config_payer.id.pubkey())]);
        let config_payer_balance = RpcRequest::GetBalance
            .make_rpc_request(&rpc_client, 1, Some(params))
            .unwrap()
            .as_u64()
            .unwrap();
        assert_eq!(config_payer_balance, 39);
        let params = json!([format!("{}", process_id)]);
        let contract_balance = RpcRequest::GetBalance
            .make_rpc_request(&rpc_client, 1, Some(params))
            .unwrap()
            .as_u64()
            .unwrap();
        assert_eq!(contract_balance, 1);
        let params = json!([format!("{}", bob_pubkey)]);
        let recipient_balance = RpcRequest::GetBalance
            .make_rpc_request(&rpc_client, 1, Some(params))
            .unwrap()
            .as_u64()
            .unwrap();
        assert_eq!(recipient_balance, 10);

        server.close().unwrap();
        remove_dir_all(ledger_path).unwrap();
    }
    #[test]
    #[ignore]
    fn test_wallet_witness_tx() {
        let bob_pubkey = Keypair::new().pubkey();
        let leader_keypair = Arc::new(Keypair::new());
        let leader = Node::new_localhost_with_pubkey(leader_keypair.pubkey());
        let leader_data = leader.info.clone();
        let (alice, ledger_path) =
            create_tmp_genesis("wallet_witness_tx", 10_000_000, leader_data.id, 1000);
        let mut bank = Bank::new(&alice);

        let mut config_payer = WalletConfig::default();
        let mut config_witness = WalletConfig::default();
        let leader_scheduler = Arc::new(RwLock::new(LeaderScheduler::from_bootstrap_leader(
            leader_data.id,
        )));
        bank.leader_scheduler = leader_scheduler;
        let vote_account_keypair = Arc::new(Keypair::new());
        let last_id = bank.last_id();
        let server = Fullnode::new_with_bank(
            leader_keypair,
            vote_account_keypair,
            bank,
            0,
            &last_id,
            leader,
            None,
            &ledger_path,
            false,
            None,
        );
        sleep(Duration::from_millis(900));

        let (sender, receiver) = channel();
        run_local_drone(alice.keypair(), sender);
        let drone_addr = receiver.recv().unwrap();

        let rpc_client = RpcClient::new_from_socket(leader_data.rpc);

        assert_ne!(config_payer.id.pubkey(), config_witness.id.pubkey());

        let last_id = get_last_id(&rpc_client).unwrap();
        let transaction =
            request_airdrop_transaction(&drone_addr, &config_payer.id.pubkey(), 50, last_id)
                .unwrap();
        send_and_confirm_tx(&rpc_client, &transaction).unwrap();

        // Make transaction (from config_payer to bob_pubkey) requiring witness signature from config_witness
        config_payer.command = WalletCommand::Pay(
            10,
            bob_pubkey,
            None,
            None,
            Some(vec![config_witness.id.pubkey()]),
            None,
        );
        let sig_response = process_command(&config_payer);
        assert!(sig_response.is_ok());

        let object: Value = serde_json::from_str(&sig_response.unwrap()).unwrap();
        let process_id_str = object.get("processId").unwrap().as_str().unwrap();
        let process_id_vec = bs58::decode(process_id_str)
            .into_vec()
            .expect("base58-encoded public key");
        let process_id = Pubkey::new(&process_id_vec);

        let params = json!([format!("{}", config_payer.id.pubkey())]);
        let config_payer_balance = RpcRequest::GetBalance
            .make_rpc_request(&rpc_client, 1, Some(params))
            .unwrap()
            .as_u64()
            .unwrap();
        assert_eq!(config_payer_balance, 39);
        let params = json!([format!("{}", process_id)]);
        let contract_balance = RpcRequest::GetBalance
            .make_rpc_request(&rpc_client, 1, Some(params))
            .unwrap()
            .as_u64()
            .unwrap();
        assert_eq!(contract_balance, 11);
        let params = json!([format!("{}", bob_pubkey)]);
        let recipient_balance = RpcRequest::GetBalance
            .make_rpc_request(&rpc_client, 1, Some(params))
            .unwrap()
            .as_u64()
            .unwrap();
        assert_eq!(recipient_balance, 0);

        // Sign transaction by config_witness
        config_witness.command = WalletCommand::Witness(bob_pubkey, process_id);
        let sig_response = process_command(&config_witness);
        assert!(sig_response.is_ok());

        let params = json!([format!("{}", config_payer.id.pubkey())]);
        let config_payer_balance = RpcRequest::GetBalance
            .make_rpc_request(&rpc_client, 1, Some(params))
            .unwrap()
            .as_u64()
            .unwrap();
        assert_eq!(config_payer_balance, 39);
        let params = json!([format!("{}", process_id)]);
        let contract_balance = RpcRequest::GetBalance
            .make_rpc_request(&rpc_client, 1, Some(params))
            .unwrap()
            .as_u64()
            .unwrap();
        assert_eq!(contract_balance, 1);
        let params = json!([format!("{}", bob_pubkey)]);
        let recipient_balance = RpcRequest::GetBalance
            .make_rpc_request(&rpc_client, 1, Some(params))
            .unwrap()
            .as_u64()
            .unwrap();
        assert_eq!(recipient_balance, 10);

        server.close().unwrap();
        remove_dir_all(ledger_path).unwrap();
    }
    #[test]
    #[ignore]
    fn test_wallet_cancel_tx() {
        let bob_pubkey = Keypair::new().pubkey();
        let leader_keypair = Arc::new(Keypair::new());
        let leader = Node::new_localhost_with_pubkey(leader_keypair.pubkey());
        let leader_data = leader.info.clone();

        let (alice, ledger_path) =
            create_tmp_genesis("wallet_cancel_tx", 10_000_000, leader_data.id, 1000);
        let mut bank = Bank::new(&alice);

        let leader_scheduler = Arc::new(RwLock::new(LeaderScheduler::from_bootstrap_leader(
            leader_data.id,
        )));
        bank.leader_scheduler = leader_scheduler;
        let vote_account_keypair = Arc::new(Keypair::new());
        let last_id = bank.last_id();
        let server = Fullnode::new_with_bank(
            leader_keypair,
            vote_account_keypair,
            bank,
            0,
            &last_id,
            leader,
            None,
            &ledger_path,
            false,
            None,
        );
        sleep(Duration::from_millis(900));

        let (sender, receiver) = channel();
        run_local_drone(alice.keypair(), sender);
        let drone_addr = receiver.recv().unwrap();

        let rpc_client = RpcClient::new_from_socket(leader_data.rpc);

        let mut config_payer = WalletConfig::default();
        config_payer.network = leader_data.ncp;
        config_payer.drone_port = Some(drone_addr.port());

        let mut config_witness = WalletConfig::default();
        config_witness.network = leader_data.ncp;
        config_witness.drone_port = Some(drone_addr.port());

        assert_ne!(config_payer.id.pubkey(), config_witness.id.pubkey());

        let last_id = get_last_id(&rpc_client).unwrap();
        let transaction =
            request_airdrop_transaction(&drone_addr, &config_payer.id.pubkey(), 50, last_id)
                .unwrap();
        send_and_confirm_tx(&rpc_client, &transaction).unwrap();

        // Make transaction (from config_payer to bob_pubkey) requiring witness signature from config_witness
        config_payer.command = WalletCommand::Pay(
            10,
            bob_pubkey,
            None,
            None,
            Some(vec![config_witness.id.pubkey()]),
            Some(config_payer.id.pubkey()),
        );
        let sig_response = process_command(&config_payer);
        assert!(sig_response.is_ok());

        let object: Value = serde_json::from_str(&sig_response.unwrap()).unwrap();
        let process_id_str = object.get("processId").unwrap().as_str().unwrap();
        let process_id_vec = bs58::decode(process_id_str)
            .into_vec()
            .expect("base58-encoded public key");
        let process_id = Pubkey::new(&process_id_vec);

        let params = json!([format!("{}", config_payer.id.pubkey())]);
        let config_payer_balance = RpcRequest::GetBalance
            .make_rpc_request(&rpc_client, 1, Some(params))
            .unwrap()
            .as_u64()
            .unwrap();
        assert_eq!(config_payer_balance, 39);
        let params = json!([format!("{}", process_id)]);
        let contract_balance = RpcRequest::GetBalance
            .make_rpc_request(&rpc_client, 1, Some(params))
            .unwrap()
            .as_u64()
            .unwrap();
        assert_eq!(contract_balance, 11);
        let params = json!([format!("{}", bob_pubkey)]);
        let recipient_balance = RpcRequest::GetBalance
            .make_rpc_request(&rpc_client, 1, Some(params))
            .unwrap()
            .as_u64()
            .unwrap();
        assert_eq!(recipient_balance, 0);

        // Sign transaction by config_witness
        config_payer.command = WalletCommand::Cancel(process_id);
        let sig_response = process_command(&config_payer);
        assert!(sig_response.is_ok());

        let params = json!([format!("{}", config_payer.id.pubkey())]);
        let config_payer_balance = RpcRequest::GetBalance
            .make_rpc_request(&rpc_client, 1, Some(params))
            .unwrap()
            .as_u64()
            .unwrap();
        assert_eq!(config_payer_balance, 49);
        let params = json!([format!("{}", process_id)]);
        let contract_balance = RpcRequest::GetBalance
            .make_rpc_request(&rpc_client, 1, Some(params))
            .unwrap()
            .as_u64()
            .unwrap();
        assert_eq!(contract_balance, 1);
        let params = json!([format!("{}", bob_pubkey)]);
        let recipient_balance = RpcRequest::GetBalance
            .make_rpc_request(&rpc_client, 1, Some(params))
            .unwrap()
            .as_u64()
            .unwrap();
        assert_eq!(recipient_balance, 0);

        server.close().unwrap();
        remove_dir_all(ledger_path).unwrap();
    }
}
