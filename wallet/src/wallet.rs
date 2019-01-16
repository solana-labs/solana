use bincode::serialize;
use bs58;
use chrono::prelude::*;
use clap::ArgMatches;
use serde_json;
use serde_json::json;
use solana::rpc::{RpcSignatureStatus, RPC_PORT};
#[cfg(test)]
use solana::rpc_mock::{request_airdrop_transaction, MockRpcClient as RpcClient};
#[cfg(not(test))]
use solana::rpc_request::RpcClient;
use solana::rpc_request::{get_rpc_request_str, RpcRequest};
use solana::socketaddr;
#[cfg(not(test))]
use solana_drone::drone::request_airdrop_transaction;
use solana_drone::drone::DRONE_PORT;
use solana_sdk::bpf_loader;
use solana_sdk::budget_program;
use solana_sdk::budget_transaction::BudgetTransaction;
use solana_sdk::hash::Hash;
use solana_sdk::loader_transaction::LoaderTransaction;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Keypair, KeypairUtil, Signature};
use solana_sdk::system_transaction::SystemTransaction;
use solana_sdk::transaction::Transaction;
use std::fs::File;
use std::io::Read;
use std::net::{Ipv4Addr, SocketAddr};
use std::str::FromStr;
use std::thread::sleep;
use std::time::Duration;
use std::{error, fmt, mem};

const USERDATA_CHUNK_SIZE: usize = 256;

#[derive(Debug, PartialEq)]
pub enum WalletCommand {
    Address,
    Airdrop(u64),
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
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid")
    }
}

impl error::Error for WalletError {
    fn description(&self) -> &str {
        "invalid"
    }

    fn cause(&self) -> Option<&dyn error::Error> {
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
    pub rpc_client: Option<RpcClient>,
    pub rpc_port: Option<u16>,
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
            rpc_client: None,
            rpc_port: None,
        }
    }
}

impl WalletConfig {
    pub fn drone_addr(&self) -> SocketAddr {
        // Assume drone is running on the provided network entrypoint
        let mut drone_addr = self.network;
        drone_addr.set_port(self.drone_port.unwrap_or(DRONE_PORT));
        drone_addr
    }

    pub fn rpc_addr(&self) -> String {
        let mut rpc_addr = self.network;
        rpc_addr.set_port(self.rpc_port.unwrap_or(RPC_PORT));
        let rpc_addr_str = get_rpc_request_str(rpc_addr);
        self.proxy.clone().unwrap_or(rpc_addr_str)
    }
}

pub fn parse_command(
    pubkey: Pubkey,
    matches: &ArgMatches<'_>,
) -> Result<WalletCommand, Box<dyn error::Error>> {
    let response = match matches.subcommand() {
        ("address", Some(_address_matches)) => Ok(WalletCommand::Address),
        ("airdrop", Some(airdrop_matches)) => {
            let tokens = airdrop_matches.value_of("tokens").unwrap().parse()?;
            Ok(WalletCommand::Airdrop(tokens))
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

pub fn process_command(config: &WalletConfig) -> Result<String, Box<dyn error::Error>> {
    if let WalletCommand::Address = config.command {
        // Get address of this client
        return Ok(format!("{}", config.id.pubkey()));
    }

    let drone_addr = config.drone_addr();
    let rpc_client = if config.rpc_client.is_none() {
        let rpc_addr = config.rpc_addr();
        RpcClient::new(rpc_addr)
    } else {
        // Primarily for testing
        config.rpc_client.clone().unwrap()
    };

    match config.command {
        // Get address of this client
        WalletCommand::Address => unreachable!(),
        // Request an airdrop from Solana Drone;
        WalletCommand::Airdrop(tokens) => {
            println!(
                "Requesting airdrop of {:?} tokens from {}",
                tokens, drone_addr
            );
            let params = json!([format!("{}", config.id.pubkey())]);
            let previous_balance = match rpc_client
                .retry_make_rpc_request(1, &RpcRequest::GetBalance, Some(params), 5)?
                .as_u64()
            {
                Some(tokens) => tokens,
                None => Err(WalletError::RpcRequestError(
                    "Received result of an unexpected type".to_string(),
                ))?,
            };

            request_and_confirm_airdrop(&rpc_client, &drone_addr, &config.id.pubkey(), tokens)
                .unwrap();

            let params = json!([format!("{}", config.id.pubkey())]);
            let current_balance = rpc_client
                .retry_make_rpc_request(1, &RpcRequest::GetBalance, Some(params), 5)?
                .as_u64()
                .unwrap_or(previous_balance);

            if current_balance < previous_balance {
                Err(format!(
                    "Airdrop failed: current_balance({}) < previous_balance({})",
                    current_balance, previous_balance
                ))?;
            }
            if current_balance - previous_balance < tokens {
                Err(format!(
                    "Airdrop failed: Account balance increased by {} instead of {}",
                    current_balance - previous_balance,
                    tokens
                ))?;
            }
            Ok(format!("Your balance is: {:?}", current_balance))
        }
        // Check client balance
        WalletCommand::Balance => {
            let params = json!([format!("{}", config.id.pubkey())]);
            let balance = rpc_client
                .retry_make_rpc_request(1, &RpcRequest::GetBalance, Some(params), 5)?
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
            let mut tx =
                Transaction::budget_new_signature(&config.id, pubkey, config.id.pubkey(), last_id);
            let signature_str = send_and_confirm_tx(&rpc_client, &mut tx, &config.id)?;
            Ok(signature_str.to_string())
        }
        // Confirm the last client transaction by signature
        WalletCommand::Confirm(signature) => {
            let params = json!([format!("{}", signature)]);
            let confirmation = rpc_client
                .retry_make_rpc_request(1, &RpcRequest::ConfirmTransaction, Some(params), 5)?
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
            let balance = rpc_client
                .retry_make_rpc_request(1, &RpcRequest::GetBalance, Some(params), 5)?
                .as_u64();
            if let Some(tokens) = balance {
                if tokens < 1 {
                    Err(WalletError::DynamicProgramError(
                        "Insufficient funds".to_string(),
                    ))?
                }
            }

            let last_id = get_last_id(&rpc_client)?;
            let program_id = Keypair::new();
            let mut file = File::open(program_location).map_err(|err| {
                WalletError::DynamicProgramError(
                    format!("Unable to open program file: {}", err).to_string(),
                )
            })?;
            let mut program_userdata = Vec::new();
            file.read_to_end(&mut program_userdata).map_err(|err| {
                WalletError::DynamicProgramError(
                    format!("Unable to read program file: {}", err).to_string(),
                )
            })?;

            let mut tx = Transaction::system_create(
                &config.id,
                program_id.pubkey(),
                last_id,
                1,
                program_userdata.len() as u64,
                bpf_loader::id(),
                0,
            );
            send_and_confirm_tx(&rpc_client, &mut tx, &config.id).map_err(|_| {
                WalletError::DynamicProgramError("Program allocate space failed".to_string())
            })?;

            let mut offset = 0;
            for chunk in program_userdata.chunks(USERDATA_CHUNK_SIZE) {
                let mut tx = Transaction::loader_write(
                    &program_id,
                    bpf_loader::id(),
                    offset,
                    chunk.to_vec(),
                    last_id,
                    0,
                );
                send_and_confirm_tx(&rpc_client, &mut tx, &program_id).map_err(|_| {
                    WalletError::DynamicProgramError(format!(
                        "Program write failed at offset {:?}",
                        offset
                    ))
                })?;
                offset += USERDATA_CHUNK_SIZE as u32;
            }

            let last_id = get_last_id(&rpc_client)?;
            let mut tx = Transaction::loader_finalize(&program_id, bpf_loader::id(), last_id, 0);
            send_and_confirm_tx(&rpc_client, &mut tx, &program_id).map_err(|_| {
                WalletError::DynamicProgramError("Program finalize transaction failed".to_string())
            })?;

            let mut tx = Transaction::system_spawn(&program_id, last_id, 0);
            send_and_confirm_tx(&rpc_client, &mut tx, &program_id).map_err(|_| {
                WalletError::DynamicProgramError("Program spawn failed".to_string())
            })?;

            Ok(json!({
                "programId": format!("{}", program_id.pubkey()),
            })
            .to_string())
        }
        WalletCommand::GetTransactionCount => {
            let transaction_count = rpc_client
                .retry_make_rpc_request(1, &RpcRequest::GetTransactionCount, None, 5)?
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
                let mut tx = Transaction::system_new(&config.id, to, tokens, last_id);
                let signature_str = send_and_confirm_tx(&rpc_client, &mut tx, &config.id)?;
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
                let mut tx = Transaction::system_create(
                    &config.id,
                    contract_funds.pubkey(),
                    last_id,
                    tokens,
                    0,
                    budget_program_id,
                    0,
                );
                send_and_confirm_tx(&rpc_client, &mut tx, &config.id)?;

                // Create account for contract state
                let mut tx = Transaction::system_create(
                    &config.id,
                    contract_state.pubkey(),
                    last_id,
                    1,
                    196,
                    budget_program_id,
                    0,
                );
                send_and_confirm_tx(&rpc_client, &mut tx, &config.id)?;

                // Initializing contract
                let mut tx = Transaction::budget_new_on_date(
                    &contract_funds,
                    to,
                    contract_state.pubkey(),
                    dt,
                    dt_pubkey,
                    cancelable,
                    tokens,
                    last_id,
                );
                let signature_str = send_and_confirm_tx(&rpc_client, &mut tx, &config.id)?;

                Ok(json!({
                    "signature": signature_str,
                    "processId": format!("{}", contract_state.pubkey()),
                })
                .to_string())
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
                let mut tx = Transaction::system_create(
                    &config.id,
                    contract_funds.pubkey(),
                    last_id,
                    tokens,
                    0,
                    budget_program_id,
                    0,
                );
                send_and_confirm_tx(&rpc_client, &mut tx, &config.id)?;

                // Create account for contract state
                let mut tx = Transaction::system_create(
                    &config.id,
                    contract_state.pubkey(),
                    last_id,
                    1,
                    196,
                    budget_program_id,
                    0,
                );
                send_and_confirm_tx(&rpc_client, &mut tx, &config.id)?;

                // Initializing contract
                let mut tx = Transaction::budget_new_when_signed(
                    &contract_funds,
                    to,
                    contract_state.pubkey(),
                    witness,
                    cancelable,
                    tokens,
                    last_id,
                );
                let signature_str = send_and_confirm_tx(&rpc_client, &mut tx, &config.id)?;

                Ok(json!({
                    "signature": signature_str,
                    "processId": format!("{}", contract_state.pubkey()),
                })
                .to_string())
            } else {
                Ok("Combo transactions not yet handled".to_string())
            }
        }
        // Apply time elapsed to contract
        WalletCommand::TimeElapsed(to, pubkey, dt) => {
            let params = json!([format!("{}", config.id.pubkey())]);
            let balance = rpc_client
                .retry_make_rpc_request(1, &RpcRequest::GetBalance, Some(params), 5)?
                .as_u64();

            if let Some(0) = balance {
                request_and_confirm_airdrop(&rpc_client, &drone_addr, &config.id.pubkey(), 1)
                    .unwrap();
            }

            let last_id = get_last_id(&rpc_client)?;

            let mut tx = Transaction::budget_new_timestamp(&config.id, pubkey, to, dt, last_id);
            let signature_str = send_and_confirm_tx(&rpc_client, &mut tx, &config.id)?;

            Ok(signature_str.to_string())
        }
        // Apply witness signature to contract
        WalletCommand::Witness(to, pubkey) => {
            let params = json!([format!("{}", config.id.pubkey())]);
            let balance = rpc_client
                .retry_make_rpc_request(1, &RpcRequest::GetBalance, Some(params), 5)?
                .as_u64();

            if let Some(0) = balance {
                request_and_confirm_airdrop(&rpc_client, &drone_addr, &config.id.pubkey(), 1)
                    .unwrap();
            }

            let last_id = get_last_id(&rpc_client)?;
            let mut tx = Transaction::budget_new_signature(&config.id, pubkey, to, last_id);
            let signature_str = send_and_confirm_tx(&rpc_client, &mut tx, &config.id)?;

            Ok(signature_str.to_string())
        }
    }
}

fn get_last_id(rpc_client: &RpcClient) -> Result<Hash, Box<dyn error::Error>> {
    let result = rpc_client.retry_make_rpc_request(1, &RpcRequest::GetLastId, None, 5)?;
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

fn send_tx(rpc_client: &RpcClient, tx: &Transaction) -> Result<String, Box<dyn error::Error>> {
    let serialized = serialize(tx).unwrap();
    let params = json!([serialized]);
    let signature =
        rpc_client.retry_make_rpc_request(2, &RpcRequest::SendTransaction, Some(params), 5)?;
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
) -> Result<RpcSignatureStatus, Box<dyn error::Error>> {
    let params = json!([signature.to_string()]);
    let signature_status =
        rpc_client.retry_make_rpc_request(1, &RpcRequest::GetSignatureStatus, Some(params), 5)?;
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

fn send_and_confirm_tx(
    rpc_client: &RpcClient,
    tx: &mut Transaction,
    signer: &Keypair,
) -> Result<String, Box<dyn error::Error>> {
    let mut send_retries = 5;
    loop {
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
            if cfg!(not(test)) {
                sleep(Duration::from_millis(500));
            }
        };
        match status {
            RpcSignatureStatus::AccountInUse | RpcSignatureStatus::SignatureNotFound => {
                // Fetch a new last_id and re-sign the transaction before sending it again
                resign_tx(rpc_client, tx, signer)?;
                send_retries -= 1;
            }
            RpcSignatureStatus::Confirmed => {
                return Ok(signature_str);
            }
            _ => {
                send_retries = 0;
            }
        }
        if send_retries == 0 {
            Err(WalletError::RpcRequestError(format!(
                "Transaction {:?} failed: {:?}",
                signature_str, status
            )))?;
        }
    }
}

fn resign_tx(
    rpc_client: &RpcClient,
    tx: &mut Transaction,
    signer_key: &Keypair,
) -> Result<(), Box<dyn error::Error>> {
    // Fetch a new last_id to prevent the retry from getting rejected as a
    // DuplicateSignature
    let mut next_last_id_retries = 3;
    let last_id = loop {
        let next_last_id = get_last_id(rpc_client)?;
        if next_last_id != tx.last_id {
            break next_last_id;
        }
        if next_last_id_retries == 0 {
            Err(WalletError::RpcRequestError(
                "Unable to fetch next last_id".to_string(),
            ))?;
        }
        next_last_id_retries -= 1;
        sleep(Duration::from_secs(1));
    };

    tx.sign(&[signer_key], last_id);
    Ok(())
}

pub fn request_and_confirm_airdrop(
    rpc_client: &RpcClient,
    drone_addr: &SocketAddr,
    id: &Pubkey,
    tokens: u64,
) -> Result<(), Box<dyn error::Error>> {
    let mut last_id = get_last_id(rpc_client)?;
    let mut send_retries = 3;
    loop {
        let tx = request_airdrop_transaction(drone_addr, id, tokens, last_id)?;
        let mut status_retries = 4;
        let signature_str = send_tx(rpc_client, &tx)?;
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
            if cfg!(not(test)) {
                sleep(Duration::from_secs(1));
            }
        };
        match status {
            RpcSignatureStatus::AccountInUse => {
                // Fetch a new last_id to prevent the retry from getting rejected as a
                // DuplicateSignature
                let mut next_last_id_retries = 3;
                loop {
                    let next_last_id = get_last_id(rpc_client)?;
                    if next_last_id != last_id {
                        last_id = next_last_id;
                        break;
                    }
                    if next_last_id_retries == 0 {
                        Err(WalletError::RpcRequestError(
                            "Unable to fetch next last_id".to_string(),
                        ))?;
                    }
                    next_last_id_retries -= 1;
                    if cfg!(not(test)) {
                        sleep(Duration::from_secs(1));
                    }
                }
                send_retries -= 1;
                if send_retries == 0 {
                    Err(WalletError::RpcRequestError(format!(
                        "AccountInUse after 3 retries: {:?}",
                        tx.account_keys[0]
                    )))?
                }
            }
            RpcSignatureStatus::Confirmed => {
                return Ok(());
            }
            _ => {
                Err(WalletError::RpcRequestError(format!(
                    "Transaction {:?} failed: {:?}",
                    signature_str, status
                )))?;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::{App, Arg, SubCommand};
    use serde_json::Value;
    use solana::rpc_mock::{PUBKEY, SIGNATURE};
    use solana_sdk::signature::{gen_keypair_file, read_keypair, read_pkcs8, Keypair, KeypairUtil};
    use std::fs;
    use std::net::{Ipv4Addr, SocketAddr};
    use std::path::Path;

    #[test]
    fn test_wallet_config_drone_addr() {
        let mut config = WalletConfig::default();
        assert_eq!(config.drone_addr(), socketaddr!(0, DRONE_PORT));

        config.drone_port = Some(1234);
        assert_eq!(config.drone_addr(), socketaddr!(0, 1234));

        assert_eq!(config.rpc_addr(), "http://0.0.0.0:8899");

        config.rpc_port = Some(1234);
        assert_eq!(config.rpc_addr(), "http://0.0.0.0:1234");
    }

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
            )
            .subcommand(SubCommand::with_name("balance").about("Get your balance"))
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
            )
            .subcommand(
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
            )
            .subcommand(
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
            )
            .subcommand(
                SubCommand::with_name("get-transaction-count")
                    .about("Get current transaction count"),
            )
            .subcommand(
                SubCommand::with_name("pay")
                    .about("Send a payment")
                    .arg(
                        Arg::with_name("to")
                            .index(1)
                            .value_name("PUBKEY")
                            .takes_value(true)
                            .required(true)
                            .help("The pubkey of recipient"),
                    )
                    .arg(
                        Arg::with_name("tokens")
                            .index(2)
                            .value_name("NUM")
                            .takes_value(true)
                            .required(true)
                            .help("The number of tokens to send"),
                    )
                    .arg(
                        Arg::with_name("timestamp")
                            .long("after")
                            .value_name("DATETIME")
                            .takes_value(true)
                            .help("A timestamp after which transaction will execute"),
                    )
                    .arg(
                        Arg::with_name("timestamp-pubkey")
                            .long("require-timestamp-from")
                            .value_name("PUBKEY")
                            .takes_value(true)
                            .requires("timestamp")
                            .help("Require timestamp from this third party"),
                    )
                    .arg(
                        Arg::with_name("witness")
                            .long("require-signature-from")
                            .value_name("PUBKEY")
                            .takes_value(true)
                            .multiple(true)
                            .use_delimiter(true)
                            .help("Any third party signatures required to unlock the tokens"),
                    )
                    .arg(
                        Arg::with_name("cancelable")
                            .long("cancelable")
                            .takes_value(false),
                    ),
            )
            .subcommand(
                SubCommand::with_name("send-signature")
                    .about("Send a signature to authorize a transfer")
                    .arg(
                        Arg::with_name("to")
                            .index(1)
                            .value_name("PUBKEY")
                            .takes_value(true)
                            .required(true)
                            .help("The pubkey of recipient"),
                    )
                    .arg(
                        Arg::with_name("process-id")
                            .index(2)
                            .value_name("PROCESS_ID")
                            .takes_value(true)
                            .required(true)
                            .help("The process id of the transfer to authorize"),
                    ),
            )
            .subcommand(
                SubCommand::with_name("send-timestamp")
                    .about("Send a timestamp to unlock a transfer")
                    .arg(
                        Arg::with_name("to")
                            .index(1)
                            .value_name("PUBKEY")
                            .takes_value(true)
                            .required(true)
                            .help("The pubkey of recipient"),
                    )
                    .arg(
                        Arg::with_name("process-id")
                            .index(2)
                            .value_name("PROCESS_ID")
                            .takes_value(true)
                            .required(true)
                            .help("The process id of the transfer to unlock"),
                    )
                    .arg(
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
            WalletCommand::Airdrop(50)
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
        // Success cases
        let mut config = WalletConfig::default();
        config.rpc_client = Some(RpcClient::new("succeeds".to_string()));

        let keypair = Keypair::new();
        let pubkey = keypair.pubkey().to_string();
        config.id = keypair;
        config.command = WalletCommand::Address;
        assert_eq!(process_command(&config).unwrap(), pubkey);

        config.command = WalletCommand::Balance;
        assert_eq!(process_command(&config).unwrap(), "Your balance is: 50");

        let process_id = Keypair::new().pubkey();
        config.command = WalletCommand::Cancel(process_id);
        assert_eq!(process_command(&config).unwrap(), SIGNATURE);

        let good_signature = Signature::new(&bs58::decode(SIGNATURE).into_vec().unwrap());
        config.command = WalletCommand::Confirm(good_signature);
        assert_eq!(process_command(&config).unwrap(), "Confirmed");
        let missing_signature = Signature::new(&bs58::decode("5VERv8NMvzbJMEkV8xnrLkEaWRtSz9CosKDYjCJjBRnbJLgp8uirBgmQpjKhoR4tjF3ZpRzrFmBV6UjKdiSZkQUW").into_vec().unwrap());
        config.command = WalletCommand::Confirm(missing_signature);
        assert_eq!(process_command(&config).unwrap(), "Not found");

        config.command = WalletCommand::GetTransactionCount;
        assert_eq!(process_command(&config).unwrap(), "1234");

        let bob_pubkey = Keypair::new().pubkey();
        config.command = WalletCommand::Pay(10, bob_pubkey, None, None, None, None);
        let signature = process_command(&config);
        assert!(signature.is_ok());
        assert_eq!(signature.unwrap(), SIGNATURE.to_string());

        let date_string = "\"2018-09-19T17:30:59Z\"";
        let dt: DateTime<Utc> = serde_json::from_str(&date_string).unwrap();
        config.command = WalletCommand::Pay(
            10,
            bob_pubkey,
            Some(dt),
            Some(config.id.pubkey()),
            None,
            None,
        );
        let result = process_command(&config);
        assert!(result.is_ok());
        let json: Value = serde_json::from_str(&result.unwrap()).unwrap();
        assert_eq!(
            json.as_object()
                .unwrap()
                .get("signature")
                .unwrap()
                .as_str()
                .unwrap(),
            SIGNATURE.to_string()
        );

        let witness = Keypair::new().pubkey();
        config.command = WalletCommand::Pay(
            10,
            bob_pubkey,
            None,
            None,
            Some(vec![witness]),
            Some(config.id.pubkey()),
        );
        let result = process_command(&config);
        assert!(result.is_ok());
        let json: Value = serde_json::from_str(&result.unwrap()).unwrap();
        assert_eq!(
            json.as_object()
                .unwrap()
                .get("signature")
                .unwrap()
                .as_str()
                .unwrap(),
            SIGNATURE.to_string()
        );

        let process_id = Keypair::new().pubkey();
        config.command = WalletCommand::TimeElapsed(bob_pubkey, process_id, dt);
        let signature = process_command(&config);
        assert!(signature.is_ok());
        assert_eq!(signature.unwrap(), SIGNATURE.to_string());

        let witness = Keypair::new().pubkey();
        config.command = WalletCommand::Witness(bob_pubkey, witness);
        let signature = process_command(&config);
        assert!(signature.is_ok());
        assert_eq!(signature.unwrap(), SIGNATURE.to_string());

        // Need airdrop cases
        config.command = WalletCommand::Airdrop(50);
        assert!(process_command(&config).is_err());

        config.rpc_client = Some(RpcClient::new("airdrop".to_string()));
        config.command = WalletCommand::TimeElapsed(bob_pubkey, process_id, dt);
        let signature = process_command(&config);
        assert!(signature.is_ok());
        assert_eq!(signature.unwrap(), SIGNATURE.to_string());

        let witness = Keypair::new().pubkey();
        config.command = WalletCommand::Witness(bob_pubkey, witness);
        let signature = process_command(&config);
        assert!(signature.is_ok());
        assert_eq!(signature.unwrap(), SIGNATURE.to_string());

        // Failture cases
        config.rpc_client = Some(RpcClient::new("fails".to_string()));

        config.command = WalletCommand::Airdrop(50);
        assert!(process_command(&config).is_err());

        config.command = WalletCommand::Balance;
        assert!(process_command(&config).is_err());

        let any_signature = Signature::new(&bs58::decode(SIGNATURE).into_vec().unwrap());
        config.command = WalletCommand::Confirm(any_signature);
        assert!(process_command(&config).is_err());

        config.command = WalletCommand::GetTransactionCount;
        assert!(process_command(&config).is_err());

        config.command = WalletCommand::Pay(10, bob_pubkey, None, None, None, None);
        assert!(process_command(&config).is_err());

        config.command = WalletCommand::Pay(
            10,
            bob_pubkey,
            Some(dt),
            Some(config.id.pubkey()),
            None,
            None,
        );
        assert!(process_command(&config).is_err());

        config.command = WalletCommand::Pay(
            10,
            bob_pubkey,
            None,
            None,
            Some(vec![witness]),
            Some(config.id.pubkey()),
        );
        assert!(process_command(&config).is_err());

        config.command = WalletCommand::TimeElapsed(bob_pubkey, process_id, dt);
        assert!(process_command(&config).is_err());
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
    fn test_wallet_get_last_id() {
        let rpc_client = RpcClient::new("succeeds".to_string());

        let vec = bs58::decode(PUBKEY).into_vec().unwrap();
        let expected_last_id = Hash::new(&vec);

        let last_id = get_last_id(&rpc_client);
        assert!(last_id.is_ok());
        assert_eq!(last_id.unwrap(), expected_last_id);

        let rpc_client = RpcClient::new("fails".to_string());

        let last_id = get_last_id(&rpc_client);
        assert!(last_id.is_err());
    }

    #[test]
    fn test_wallet_send_tx() {
        let rpc_client = RpcClient::new("succeeds".to_string());

        let key = Keypair::new();
        let to = Keypair::new().pubkey();
        let last_id = Hash::default();
        let tx = Transaction::system_new(&key, to, 50, last_id);

        let signature = send_tx(&rpc_client, &tx);
        assert!(signature.is_ok());
        assert_eq!(signature.unwrap(), SIGNATURE.to_string());

        let rpc_client = RpcClient::new("fails".to_string());

        let signature = send_tx(&rpc_client, &tx);
        assert!(signature.is_err());
    }

    #[test]
    fn test_wallet_confirm_tx() {
        let rpc_client = RpcClient::new("succeeds".to_string());
        let signature = "good_signature";
        let status = confirm_tx(&rpc_client, &signature);
        assert!(status.is_ok());
        assert_eq!(status.unwrap(), RpcSignatureStatus::Confirmed);

        let rpc_client = RpcClient::new("bad_sig_status".to_string());
        let signature = "bad_status";
        let status = confirm_tx(&rpc_client, &signature);
        assert!(status.is_err());

        let rpc_client = RpcClient::new("fails".to_string());
        let signature = "bad_status_fmt";
        let status = confirm_tx(&rpc_client, &signature);
        assert!(status.is_err());
    }

    #[test]
    fn test_wallet_send_and_confirm_tx() {
        let rpc_client = RpcClient::new("succeeds".to_string());

        let key = Keypair::new();
        let to = Keypair::new().pubkey();
        let last_id = Hash::default();
        let mut tx = Transaction::system_new(&key, to, 50, last_id);

        let signer = Keypair::new();

        let result = send_and_confirm_tx(&rpc_client, &mut tx, &signer);
        assert!(result.is_ok());

        let rpc_client = RpcClient::new("account_in_use".to_string());
        let result = send_and_confirm_tx(&rpc_client, &mut tx, &signer);
        assert!(result.is_err());

        let rpc_client = RpcClient::new("fails".to_string());
        let result = send_and_confirm_tx(&rpc_client, &mut tx, &signer);
        assert!(result.is_err());
    }

    #[test]
    fn test_wallet_resign_tx() {
        let rpc_client = RpcClient::new("succeeds".to_string());

        let key = Keypair::new();
        let to = Keypair::new().pubkey();
        let vec = bs58::decode("HUu3LwEzGRsUkuJS121jzkPJW39Kq62pXCTmTa1F9jDL")
            .into_vec()
            .unwrap();
        let last_id = Hash::new(&vec);
        let prev_tx = Transaction::system_new(&key, to, 50, last_id);
        let mut tx = Transaction::system_new(&key, to, 50, last_id);

        resign_tx(&rpc_client, &mut tx, &key).unwrap();

        assert_ne!(prev_tx, tx);
        assert_ne!(prev_tx.signatures, tx.signatures);
        assert_ne!(prev_tx.last_id, tx.last_id);
        assert_eq!(prev_tx.fee, tx.fee);
        assert_eq!(prev_tx.account_keys, tx.account_keys);
        assert_eq!(prev_tx.instructions, tx.instructions);
    }

    #[test]
    fn test_request_and_confirm_airdrop() {
        let rpc_client = RpcClient::new("succeeds".to_string());
        let drone_addr = socketaddr!(0, 0);
        let id = Keypair::new().pubkey();
        let tokens = 50;
        assert_eq!(
            request_and_confirm_airdrop(&rpc_client, &drone_addr, &id, tokens).unwrap(),
            ()
        );

        let rpc_client = RpcClient::new("account_in_use".to_string());
        assert!(request_and_confirm_airdrop(&rpc_client, &drone_addr, &id, tokens).is_err());

        let tokens = 0;
        assert!(request_and_confirm_airdrop(&rpc_client, &drone_addr, &id, tokens).is_err());
    }
}
