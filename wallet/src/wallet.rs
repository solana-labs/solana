use bs58;
use chrono::prelude::*;
use clap::ArgMatches;
use log::*;
use serde_json;
use serde_json::json;
use solana_budget_api;
use solana_budget_api::budget_instruction;
use solana_client::rpc_client::{get_rpc_request_str, RpcClient};
#[cfg(not(test))]
use solana_drone::drone::request_airdrop_transaction;
use solana_drone::drone::DRONE_PORT;
#[cfg(test)]
use solana_drone::drone_mock::request_airdrop_transaction;
use solana_sdk::bpf_loader;
use solana_sdk::hash::Hash;
use solana_sdk::loader_instruction;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::rpc_port::DEFAULT_RPC_PORT;
use solana_sdk::signature::{Keypair, KeypairUtil, Signature};
use solana_sdk::system_transaction;
use solana_sdk::transaction::Transaction;
use solana_vote_api::vote_instruction;
use std::fs::File;
use std::io::Read;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::{error, fmt, mem};

const USERDATA_CHUNK_SIZE: usize = 256;

#[derive(Debug, PartialEq)]
pub enum WalletCommand {
    Address,
    Airdrop(u64),
    Balance(Pubkey),
    Cancel(Pubkey),
    Confirm(Signature),
    // ConfigureStakingAccount(delegate_id, authorized_voter_id)
    AuthorizeVoter(Pubkey),
    CreateVoteAccount(Pubkey, Pubkey, u32, u64),
    ShowVoteAccount(Pubkey),
    Deploy(String),
    GetTransactionCount,
    // Pay(lamports, to, timestamp, timestamp_pubkey, witness(es), cancelable)
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
    pub keypair: Keypair,
    pub command: WalletCommand,
    pub drone_host: Option<IpAddr>,
    pub drone_port: u16,
    pub host: IpAddr,
    pub rpc_client: Option<RpcClient>,
    pub rpc_host: Option<IpAddr>,
    pub rpc_port: u16,
    pub rpc_tls: bool,
}

impl Default for WalletConfig {
    fn default() -> WalletConfig {
        WalletConfig {
            command: WalletCommand::Balance(Pubkey::default()),
            drone_host: None,
            drone_port: DRONE_PORT,
            host: IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
            keypair: Keypair::new(),
            rpc_client: None,
            rpc_host: None,
            rpc_port: DEFAULT_RPC_PORT,
            rpc_tls: false,
        }
    }
}

impl WalletConfig {
    pub fn drone_addr(&self) -> SocketAddr {
        SocketAddr::new(self.drone_host.unwrap_or(self.host), self.drone_port)
    }

    pub fn rpc_addr(&self) -> String {
        get_rpc_request_str(
            SocketAddr::new(self.rpc_host.unwrap_or(self.host), self.rpc_port),
            self.rpc_tls,
        )
    }
}

// Return the pubkey for an argument with `name` or None if not present.
fn pubkey_of(matches: &ArgMatches<'_>, name: &str) -> Option<Pubkey> {
    matches.value_of(name).map(|x| x.parse::<Pubkey>().unwrap())
}

// Return the pubkeys for arguments with `name` or None if none present.
fn pubkeys_of(matches: &ArgMatches<'_>, name: &str) -> Option<Vec<Pubkey>> {
    matches
        .values_of(name)
        .map(|xs| xs.map(|x| x.parse::<Pubkey>().unwrap()).collect())
}

pub fn parse_command(
    pubkey: &Pubkey,
    matches: &ArgMatches<'_>,
) -> Result<WalletCommand, Box<dyn error::Error>> {
    let response = match matches.subcommand() {
        ("address", Some(_address_matches)) => Ok(WalletCommand::Address),
        ("airdrop", Some(airdrop_matches)) => {
            let lamports = airdrop_matches.value_of("lamports").unwrap().parse()?;
            Ok(WalletCommand::Airdrop(lamports))
        }
        ("balance", Some(balance_matches)) => {
            let pubkey = pubkey_of(&balance_matches, "pubkey").unwrap_or(*pubkey);
            Ok(WalletCommand::Balance(pubkey))
        }
        ("cancel", Some(cancel_matches)) => {
            let process_id = pubkey_of(cancel_matches, "process_id").unwrap();
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
        ("authorize-voter", Some(matches)) => {
            let authorized_voter_id = pubkey_of(matches, "authorized_voter_id").unwrap();
            Ok(WalletCommand::AuthorizeVoter(authorized_voter_id))
        }
        ("create-vote-account", Some(matches)) => {
            let voting_account_id = pubkey_of(matches, "voting_account_id").unwrap();
            let node_id = pubkey_of(matches, "node_id").unwrap();
            let commission = if let Some(commission) = matches.value_of("commission") {
                commission.parse()?
            } else {
                0
            };
            let lamports = matches.value_of("lamports").unwrap().parse()?;
            Ok(WalletCommand::CreateVoteAccount(
                voting_account_id,
                node_id,
                commission,
                lamports,
            ))
        }
        ("show-vote-account", Some(matches)) => {
            let voting_account_id = pubkey_of(matches, "voting_account_id").unwrap();
            Ok(WalletCommand::ShowVoteAccount(voting_account_id))
        }
        ("deploy", Some(deploy_matches)) => Ok(WalletCommand::Deploy(
            deploy_matches
                .value_of("program_location")
                .unwrap()
                .to_string(),
        )),
        ("get-transaction-count", Some(_matches)) => Ok(WalletCommand::GetTransactionCount),
        ("pay", Some(pay_matches)) => {
            let lamports = pay_matches.value_of("lamports").unwrap().parse()?;
            let to = pubkey_of(&pay_matches, "to").unwrap_or(*pubkey);
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
            let timestamp_pubkey = pubkey_of(&pay_matches, "timestamp_pubkey");
            let witness_vec = pubkeys_of(&pay_matches, "witness");
            let cancelable = if pay_matches.is_present("cancelable") {
                Some(*pubkey)
            } else {
                None
            };

            Ok(WalletCommand::Pay(
                lamports,
                to,
                timestamp,
                timestamp_pubkey,
                witness_vec,
                cancelable,
            ))
        }
        ("send-signature", Some(sig_matches)) => {
            let to = pubkey_of(&sig_matches, "to").unwrap();
            let process_id = pubkey_of(&sig_matches, "process_id").unwrap();
            Ok(WalletCommand::Witness(to, process_id))
        }
        ("send-timestamp", Some(timestamp_matches)) => {
            let to = pubkey_of(&timestamp_matches, "to").unwrap();
            let process_id = pubkey_of(&timestamp_matches, "process_id").unwrap();
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

type ProcessResult = Result<String, Box<dyn error::Error>>;

fn process_airdrop(
    rpc_client: &RpcClient,
    config: &WalletConfig,
    drone_addr: SocketAddr,
    lamports: u64,
) -> ProcessResult {
    println!(
        "Requesting airdrop of {:?} lamports from {}",
        lamports, drone_addr
    );
    let previous_balance = match rpc_client.retry_get_balance(&config.keypair.pubkey(), 5)? {
        Some(lamports) => lamports,
        None => Err(WalletError::RpcRequestError(
            "Received result of an unexpected type".to_string(),
        ))?,
    };

    request_and_confirm_airdrop(&rpc_client, &drone_addr, &config.keypair.pubkey(), lamports)?;

    let current_balance = rpc_client
        .retry_get_balance(&config.keypair.pubkey(), 5)?
        .unwrap_or(previous_balance);

    if current_balance < previous_balance {
        Err(format!(
            "Airdrop failed: current_balance({}) < previous_balance({})",
            current_balance, previous_balance
        ))?;
    }
    if current_balance - previous_balance < lamports {
        Err(format!(
            "Airdrop failed: Account balance increased by {} instead of {}",
            current_balance - previous_balance,
            lamports
        ))?;
    }
    Ok(format!("Your balance is: {:?}", current_balance))
}

fn process_balance(pubkey: &Pubkey, rpc_client: &RpcClient) -> ProcessResult {
    let balance = rpc_client.retry_get_balance(pubkey, 5)?;
    match balance {
        Some(lamports) => {
            let ess = if lamports == 1 { "" } else { "s" };
            Ok(format!("{:?} lamport{}", lamports, ess))
        }
        None => Err(WalletError::RpcRequestError(
            "Received result of an unexpected type".to_string(),
        ))?,
    }
}

fn process_confirm(rpc_client: &RpcClient, signature: Signature) -> ProcessResult {
    match rpc_client.get_signature_status(&signature.to_string()) {
        Ok(status) => {
            if let Some(result) = status {
                match result {
                    Ok(_) => Ok("Confirmed".to_string()),
                    Err(err) => Ok(format!("Transaction failed with error {:?}", err)),
                }
            } else {
                Ok("Not found".to_string())
            }
        }
        Err(err) => Err(WalletError::RpcRequestError(format!(
            "Unable to confirm: {:?}",
            err
        )))?,
    }
}

fn process_authorize_voter(
    rpc_client: &RpcClient,
    config: &WalletConfig,
    authorized_voter_id: Pubkey,
) -> ProcessResult {
    let recent_blockhash = rpc_client.get_recent_blockhash()?;
    let ixs = vec![vote_instruction::authorize_voter(
        &config.keypair.pubkey(),
        &authorized_voter_id,
    )];

    let mut tx = Transaction::new_signed_instructions(&[&config.keypair], ixs, recent_blockhash);
    let signature_str = rpc_client.send_and_confirm_transaction(&mut tx, &config.keypair)?;
    Ok(signature_str.to_string())
}

fn process_create_staking(
    rpc_client: &RpcClient,
    config: &WalletConfig,
    voting_account_id: &Pubkey,
    node_id: &Pubkey,
    commission: u32,
    lamports: u64,
) -> ProcessResult {
    let recent_blockhash = rpc_client.get_recent_blockhash()?;
    let ixs = vote_instruction::create_account(
        &config.keypair.pubkey(),
        voting_account_id,
        node_id,
        commission,
        lamports,
    );
    let mut tx = Transaction::new_signed_instructions(&[&config.keypair], ixs, recent_blockhash);
    let signature_str = rpc_client.send_and_confirm_transaction(&mut tx, &config.keypair)?;
    Ok(signature_str.to_string())
}

fn process_show_staking(
    rpc_client: &RpcClient,
    _config: &WalletConfig,
    voting_account_id: &Pubkey,
) -> ProcessResult {
    use solana_vote_api::vote_state::VoteState;
    let vote_account_lamports = rpc_client.retry_get_balance(voting_account_id, 5)?;
    let vote_account_data = rpc_client.get_account_data(voting_account_id)?;
    let vote_state = VoteState::deserialize(&vote_account_data).unwrap();

    println!("account lamports: {}", vote_account_lamports.unwrap());
    println!("node id: {}", vote_state.node_id);
    println!("authorized voter id: {}", vote_state.authorized_voter_id);
    println!("credits: {}", vote_state.credits());
    println!(
        "commission: {}%",
        f64::from(vote_state.commission) / f64::from(std::u32::MAX)
    );
    println!(
        "root slot: {}",
        match vote_state.root_slot {
            Some(slot) => slot.to_string(),
            None => "~".to_string(),
        }
    );
    if !vote_state.votes.is_empty() {
        println!("votes:");
        for vote in vote_state.votes {
            println!(
                "- slot={}, confirmation count={}",
                vote.slot, vote.confirmation_count
            );
        }
    }
    Ok("".to_string())
}

fn process_deploy(
    rpc_client: &RpcClient,
    config: &WalletConfig,
    program_location: &str,
) -> ProcessResult {
    let balance = rpc_client.retry_get_balance(&config.keypair.pubkey(), 5)?;
    if let Some(lamports) = balance {
        if lamports < 1 {
            Err(WalletError::DynamicProgramError(
                "Insufficient funds".to_string(),
            ))?
        }
    }

    let blockhash = rpc_client.get_recent_blockhash()?;
    let program_id = Keypair::new();
    let mut file = File::open(program_location).map_err(|err| {
        WalletError::DynamicProgramError(
            format!("Unable to open program file: {}", err).to_string(),
        )
    })?;
    let mut program_data = Vec::new();
    file.read_to_end(&mut program_data).map_err(|err| {
        WalletError::DynamicProgramError(
            format!("Unable to read program file: {}", err).to_string(),
        )
    })?;

    let mut tx = system_transaction::create_account(
        &config.keypair,
        &program_id.pubkey(),
        blockhash,
        1,
        program_data.len() as u64,
        &bpf_loader::id(),
        0,
    );
    trace!("Creating program account");
    rpc_client
        .send_and_confirm_transaction(&mut tx, &config.keypair)
        .map_err(|_| {
            WalletError::DynamicProgramError("Program allocate space failed".to_string())
        })?;

    trace!("Writing program data");
    let write_transactions: Vec<_> = program_data
        .chunks(USERDATA_CHUNK_SIZE)
        .zip(0..)
        .map(|(chunk, i)| {
            let instruction = loader_instruction::write(
                &program_id.pubkey(),
                &bpf_loader::id(),
                (i * USERDATA_CHUNK_SIZE) as u32,
                chunk.to_vec(),
            );
            Transaction::new_signed_instructions(&[&program_id], vec![instruction], blockhash)
        })
        .collect();
    rpc_client.send_and_confirm_transactions(write_transactions, &program_id)?;

    trace!("Finalizing program account");
    let instruction = loader_instruction::finalize(&program_id.pubkey(), &bpf_loader::id());
    let mut tx = Transaction::new_signed_instructions(&[&program_id], vec![instruction], blockhash);
    rpc_client
        .send_and_confirm_transaction(&mut tx, &program_id)
        .map_err(|_| {
            WalletError::DynamicProgramError("Program finalize transaction failed".to_string())
        })?;

    Ok(json!({
        "programId": format!("{}", program_id.pubkey()),
    })
    .to_string())
}

fn process_pay(
    rpc_client: &RpcClient,
    config: &WalletConfig,
    lamports: u64,
    to: &Pubkey,
    timestamp: Option<DateTime<Utc>>,
    timestamp_pubkey: Option<Pubkey>,
    witnesses: &Option<Vec<Pubkey>>,
    cancelable: Option<Pubkey>,
) -> ProcessResult {
    let blockhash = rpc_client.get_recent_blockhash()?;

    if timestamp == None && *witnesses == None {
        let mut tx = system_transaction::transfer(&config.keypair, to, lamports, blockhash, 0);
        let signature_str = rpc_client.send_and_confirm_transaction(&mut tx, &config.keypair)?;
        Ok(signature_str.to_string())
    } else if *witnesses == None {
        let dt = timestamp.unwrap();
        let dt_pubkey = match timestamp_pubkey {
            Some(pubkey) => pubkey,
            None => config.keypair.pubkey(),
        };

        let contract_state = Keypair::new();

        // Initializing contract
        let ixs = budget_instruction::on_date(
            &config.keypair.pubkey(),
            to,
            &contract_state.pubkey(),
            dt,
            &dt_pubkey,
            cancelable,
            lamports,
        );
        let mut tx = Transaction::new_signed_instructions(&[&config.keypair], ixs, blockhash);
        let signature_str = rpc_client.send_and_confirm_transaction(&mut tx, &config.keypair)?;

        Ok(json!({
            "signature": signature_str,
            "processId": format!("{}", contract_state.pubkey()),
        })
        .to_string())
    } else if timestamp == None {
        let blockhash = rpc_client.get_recent_blockhash()?;

        let witness = if let Some(ref witness_vec) = *witnesses {
            witness_vec[0]
        } else {
            Err(WalletError::BadParameter(
                "Could not parse required signature pubkey(s)".to_string(),
            ))?
        };

        let contract_state = Keypair::new();

        // Initializing contract
        let ixs = budget_instruction::when_signed(
            &config.keypair.pubkey(),
            to,
            &contract_state.pubkey(),
            &witness,
            cancelable,
            lamports,
        );
        let mut tx = Transaction::new_signed_instructions(&[&config.keypair], ixs, blockhash);
        let signature_str = rpc_client.send_and_confirm_transaction(&mut tx, &config.keypair)?;

        Ok(json!({
            "signature": signature_str,
            "processId": format!("{}", contract_state.pubkey()),
        })
        .to_string())
    } else {
        Ok("Combo transactions not yet handled".to_string())
    }
}

fn process_cancel(rpc_client: &RpcClient, config: &WalletConfig, pubkey: &Pubkey) -> ProcessResult {
    let blockhash = rpc_client.get_recent_blockhash()?;
    let ix = budget_instruction::apply_signature(
        &config.keypair.pubkey(),
        pubkey,
        &config.keypair.pubkey(),
    );
    let mut tx = Transaction::new_signed_instructions(&[&config.keypair], vec![ix], blockhash);
    let signature_str = rpc_client.send_and_confirm_transaction(&mut tx, &config.keypair)?;
    Ok(signature_str.to_string())
}

fn process_get_transaction_count(rpc_client: &RpcClient) -> ProcessResult {
    let transaction_count = rpc_client.get_transaction_count()?;
    Ok(transaction_count.to_string())
}

fn process_time_elapsed(
    rpc_client: &RpcClient,
    config: &WalletConfig,
    drone_addr: SocketAddr,
    to: &Pubkey,
    pubkey: &Pubkey,
    dt: DateTime<Utc>,
) -> ProcessResult {
    let balance = rpc_client.retry_get_balance(&config.keypair.pubkey(), 5)?;

    if let Some(0) = balance {
        request_and_confirm_airdrop(&rpc_client, &drone_addr, &config.keypair.pubkey(), 1)?;
    }

    let blockhash = rpc_client.get_recent_blockhash()?;

    let ix = budget_instruction::apply_timestamp(&config.keypair.pubkey(), pubkey, to, dt);
    let mut tx = Transaction::new_signed_instructions(&[&config.keypair], vec![ix], blockhash);
    let signature_str = rpc_client.send_and_confirm_transaction(&mut tx, &config.keypair)?;

    Ok(signature_str.to_string())
}

fn process_witness(
    rpc_client: &RpcClient,
    config: &WalletConfig,
    drone_addr: SocketAddr,
    to: &Pubkey,
    pubkey: &Pubkey,
) -> ProcessResult {
    let balance = rpc_client.retry_get_balance(&config.keypair.pubkey(), 5)?;

    if let Some(0) = balance {
        request_and_confirm_airdrop(&rpc_client, &drone_addr, &config.keypair.pubkey(), 1)?;
    }

    let blockhash = rpc_client.get_recent_blockhash()?;
    let ix = budget_instruction::apply_signature(&config.keypair.pubkey(), pubkey, to);
    let mut tx = Transaction::new_signed_instructions(&[&config.keypair], vec![ix], blockhash);
    let signature_str = rpc_client.send_and_confirm_transaction(&mut tx, &config.keypair)?;

    Ok(signature_str.to_string())
}

pub fn process_command(config: &WalletConfig) -> ProcessResult {
    if let WalletCommand::Address = config.command {
        // Get address of this client
        return Ok(format!("{}", config.keypair.pubkey()));
    }

    let drone_addr = config.drone_addr();

    let mut _rpc_client;
    let rpc_client = if config.rpc_client.is_none() {
        let rpc_addr = config.rpc_addr();
        _rpc_client = RpcClient::new(rpc_addr);
        &_rpc_client
    } else {
        // Primarily for testing
        config.rpc_client.as_ref().unwrap()
    };

    match config.command {
        // Get address of this client
        WalletCommand::Address => unreachable!(),

        // Request an airdrop from Solana Drone;
        WalletCommand::Airdrop(lamports) => {
            process_airdrop(&rpc_client, config, drone_addr, lamports)
        }

        // Check client balance
        WalletCommand::Balance(pubkey) => process_balance(&pubkey, &rpc_client),

        // Cancel a contract by contract Pubkey
        WalletCommand::Cancel(pubkey) => process_cancel(&rpc_client, config, &pubkey),

        // Confirm the last client transaction by signature
        WalletCommand::Confirm(signature) => process_confirm(&rpc_client, signature),

        // Configure staking account already created
        WalletCommand::AuthorizeVoter(authorized_voter_id) => {
            process_authorize_voter(&rpc_client, config, authorized_voter_id)
        }

        // Create staking account
        WalletCommand::CreateVoteAccount(voting_account_id, node_id, commission, lamports) => {
            process_create_staking(
                &rpc_client,
                config,
                &voting_account_id,
                &node_id,
                commission,
                lamports,
            )
        }

        WalletCommand::ShowVoteAccount(voting_account_id) => {
            process_show_staking(&rpc_client, config, &voting_account_id)
        }

        // Deploy a custom program to the chain
        WalletCommand::Deploy(ref program_location) => {
            process_deploy(&rpc_client, config, program_location)
        }

        WalletCommand::GetTransactionCount => process_get_transaction_count(&rpc_client),

        // If client has positive balance, pay lamports to another address
        WalletCommand::Pay(
            lamports,
            to,
            timestamp,
            timestamp_pubkey,
            ref witnesses,
            cancelable,
        ) => process_pay(
            &rpc_client,
            config,
            lamports,
            &to,
            timestamp,
            timestamp_pubkey,
            witnesses,
            cancelable,
        ),

        // Apply time elapsed to contract
        WalletCommand::TimeElapsed(to, pubkey, dt) => {
            process_time_elapsed(&rpc_client, config, drone_addr, &to, &pubkey, dt)
        }

        // Apply witness signature to contract
        WalletCommand::Witness(to, pubkey) => {
            process_witness(&rpc_client, config, drone_addr, &to, &pubkey)
        }
    }
}

// Quick and dirty Keypair that assumes the client will do retries but not update the
// blockhash. If the client updates the blockhash, the signature will be invalid.
// TODO: Parse `msg` and use that data to make a new airdrop request.
struct DroneKeypair {
    transaction: Transaction,
}

impl DroneKeypair {
    fn new_keypair(
        drone_addr: &SocketAddr,
        to_pubkey: &Pubkey,
        lamports: u64,
        blockhash: Hash,
    ) -> Result<Self, Box<dyn error::Error>> {
        let transaction = request_airdrop_transaction(drone_addr, to_pubkey, lamports, blockhash)?;
        Ok(Self { transaction })
    }

    fn airdrop_transaction(&self) -> Transaction {
        self.transaction.clone()
    }
}

impl KeypairUtil for DroneKeypair {
    fn new() -> Self {
        unimplemented!();
    }

    /// Return the public key of the keypair used to sign votes
    fn pubkey(&self) -> Pubkey {
        self.transaction.message().account_keys[0]
    }

    fn sign_message(&self, _msg: &[u8]) -> Signature {
        self.transaction.signatures[0]
    }
}

pub fn request_and_confirm_airdrop(
    rpc_client: &RpcClient,
    drone_addr: &SocketAddr,
    to_pubkey: &Pubkey,
    lamports: u64,
) -> Result<(), Box<dyn error::Error>> {
    let blockhash = rpc_client.get_recent_blockhash()?;
    let keypair = DroneKeypair::new_keypair(drone_addr, to_pubkey, lamports, blockhash)?;
    let mut tx = keypair.airdrop_transaction();
    rpc_client.send_and_confirm_transaction(&mut tx, &keypair)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::{App, Arg, SubCommand};
    use serde_json::Value;
    use solana_client::mock_rpc_client_request::SIGNATURE;
    use solana_sdk::transaction::TransactionError;
    use std::net::{Ipv4Addr, SocketAddr};
    use std::path::PathBuf;

    #[test]
    fn test_wallet_config_drone_addr() {
        let mut config = WalletConfig::default();
        assert_eq!(
            config.drone_addr(),
            SocketAddr::new(config.host, config.drone_port)
        );

        config.drone_port = 1234;
        assert_eq!(config.drone_addr(), SocketAddr::new(config.host, 1234));

        config.drone_host = Some(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 2)));
        assert_eq!(
            config.drone_addr(),
            SocketAddr::new(config.drone_host.unwrap(), 1234)
        );
    }

    #[test]
    fn test_wallet_config_rpc_addr() {
        let mut config = WalletConfig::default();
        assert_eq!(config.rpc_addr(), "http://127.0.0.1:8899");
        config.rpc_port = 1234;
        assert_eq!(config.rpc_addr(), "http://127.0.0.1:1234");
        config.rpc_host = Some(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 2)));
        assert_eq!(config.rpc_addr(), "http://127.0.0.2:1234");
    }

    #[test]
    fn test_wallet_parse_command() {
        let test_commands = App::new("test")
            .subcommand(SubCommand::with_name("address").about("Get your public key"))
            .subcommand(
                SubCommand::with_name("airdrop")
                    .about("Request a batch of lamports")
                    .arg(
                        Arg::with_name("lamports")
                            .index(1)
                            .value_name("NUM")
                            .takes_value(true)
                            .required(true)
                            .help("The number of lamports to request"),
                    ),
            )
            .subcommand(SubCommand::with_name("balance").about("Get your balance"))
            .subcommand(
                SubCommand::with_name("cancel")
                    .about("Cancel a transfer")
                    .arg(
                        Arg::with_name("process_id")
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
                SubCommand::with_name("authorize-voter")
                    .about("Configure staking account for node")
                    .arg(
                        Arg::with_name("authorized_voter_id")
                            .index(1)
                            .value_name("PUBKEY")
                            .takes_value(true)
                            .required(true)
                            .help("Address to delegate this vote account to"),
                    ),
            )
            .subcommand(
                SubCommand::with_name("create-vote-account")
                    .about("Create staking account for node")
                    .arg(
                        Arg::with_name("voting_account_id")
                            .index(1)
                            .value_name("PUBKEY")
                            .takes_value(true)
                            .required(true)
                            .help("Staking account address to fund"),
                    )
                    .arg(
                        Arg::with_name("node_id")
                            .index(2)
                            .value_name("PUBKEY")
                            .takes_value(true)
                            .required(true)
                            .help("Node that will vote in this account"),
                    )
                    .arg(
                        Arg::with_name("lamports")
                            .index(3)
                            .value_name("NUM")
                            .takes_value(true)
                            .required(true)
                            .help("The number of lamports to send to staking account"),
                    )
                    .arg(
                        Arg::with_name("commission")
                            .long("commission")
                            .value_name("NUM")
                            .takes_value(true)
                            .help("The commission taken on reward redemption"),
                    ),
            )
            .subcommand(
                SubCommand::with_name("deploy")
                    .about("Deploy a program")
                    .arg(
                        Arg::with_name("program_location")
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
                        Arg::with_name("lamports")
                            .index(2)
                            .value_name("NUM")
                            .takes_value(true)
                            .required(true)
                            .help("The number of lamports to send"),
                    )
                    .arg(
                        Arg::with_name("timestamp")
                            .long("after")
                            .value_name("DATETIME")
                            .takes_value(true)
                            .help("A timestamp after which transaction will execute"),
                    )
                    .arg(
                        Arg::with_name("timestamp_pubkey")
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
                            .help("Any third party signatures required to unlock the lamports"),
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
                        Arg::with_name("process_id")
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
                        Arg::with_name("process_id")
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
        let pubkey = Pubkey::new_rand();
        let pubkey_string = format!("{}", pubkey);
        let witness0 = Pubkey::new_rand();
        let witness0_string = format!("{}", witness0);
        let witness1 = Pubkey::new_rand();
        let witness1_string = format!("{}", witness1);
        let dt = Utc.ymd(2018, 9, 19).and_hms(17, 30, 59);

        // Test Airdrop Subcommand
        let test_airdrop = test_commands
            .clone()
            .get_matches_from(vec!["test", "airdrop", "50"]);
        assert_eq!(
            parse_command(&pubkey, &test_airdrop).unwrap(),
            WalletCommand::Airdrop(50)
        );
        let test_bad_airdrop = test_commands
            .clone()
            .get_matches_from(vec!["test", "airdrop", "notint"]);
        assert!(parse_command(&pubkey, &test_bad_airdrop).is_err());

        // Test Cancel Subcommand
        let test_cancel =
            test_commands
                .clone()
                .get_matches_from(vec!["test", "cancel", &pubkey_string]);
        assert_eq!(
            parse_command(&pubkey, &test_cancel).unwrap(),
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
            parse_command(&pubkey, &test_confirm).unwrap(),
            WalletCommand::Confirm(signature)
        );
        let test_bad_signature = test_commands
            .clone()
            .get_matches_from(vec!["test", "confirm", "deadbeef"]);
        assert!(parse_command(&pubkey, &test_bad_signature).is_err());

        // Test AuthorizeVoter Subcommand
        let test_authorize_voter =
            test_commands
                .clone()
                .get_matches_from(vec!["test", "authorize-voter", &pubkey_string]);
        assert_eq!(
            parse_command(&pubkey, &test_authorize_voter).unwrap(),
            WalletCommand::AuthorizeVoter(pubkey)
        );

        // Test CreateVoteAccount SubCommand
        let node_id = Pubkey::new_rand();
        let node_id_string = format!("{}", node_id);
        let test_create_vote_account = test_commands.clone().get_matches_from(vec![
            "test",
            "create-vote-account",
            &pubkey_string,
            &node_id_string,
            "50",
            "--commission",
            "10",
        ]);
        assert_eq!(
            parse_command(&pubkey, &test_create_vote_account).unwrap(),
            WalletCommand::CreateVoteAccount(pubkey, node_id, 10, 50)
        );
        let test_create_vote_account2 = test_commands.clone().get_matches_from(vec![
            "test",
            "create-vote-account",
            &pubkey_string,
            &node_id_string,
            "50",
        ]);
        assert_eq!(
            parse_command(&pubkey, &test_create_vote_account2).unwrap(),
            WalletCommand::CreateVoteAccount(pubkey, node_id, 0, 50)
        );

        // Test Deploy Subcommand
        let test_deploy =
            test_commands
                .clone()
                .get_matches_from(vec!["test", "deploy", "/Users/test/program.o"]);
        assert_eq!(
            parse_command(&pubkey, &test_deploy).unwrap(),
            WalletCommand::Deploy("/Users/test/program.o".to_string())
        );

        // Test Simple Pay Subcommand
        let test_pay =
            test_commands
                .clone()
                .get_matches_from(vec!["test", "pay", &pubkey_string, "50"]);
        assert_eq!(
            parse_command(&pubkey, &test_pay).unwrap(),
            WalletCommand::Pay(50, pubkey, None, None, None, None)
        );

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
            parse_command(&pubkey, &test_pay_multiple_witnesses).unwrap(),
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
            parse_command(&pubkey, &test_pay_single_witness).unwrap(),
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
            parse_command(&pubkey, &test_pay_timestamp).unwrap(),
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
            parse_command(&pubkey, &test_send_signature).unwrap(),
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
            parse_command(&pubkey, &test_pay_multiple_witnesses).unwrap(),
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
            parse_command(&pubkey, &test_send_timestamp).unwrap(),
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
        assert!(parse_command(&pubkey, &test_bad_timestamp).is_err());
    }

    #[test]
    fn test_wallet_process_command() {
        // Success cases
        let mut config = WalletConfig::default();
        config.rpc_client = Some(RpcClient::new_mock("succeeds".to_string()));

        let keypair = Keypair::new();
        let pubkey = keypair.pubkey().to_string();
        config.keypair = keypair;
        config.command = WalletCommand::Address;
        assert_eq!(process_command(&config).unwrap(), pubkey);

        config.command = WalletCommand::Balance(config.keypair.pubkey());
        assert_eq!(process_command(&config).unwrap(), "50 lamports");

        let process_id = Pubkey::new_rand();
        config.command = WalletCommand::Cancel(process_id);
        assert_eq!(process_command(&config).unwrap(), SIGNATURE);

        let good_signature = Signature::new(&bs58::decode(SIGNATURE).into_vec().unwrap());
        config.command = WalletCommand::Confirm(good_signature);
        assert_eq!(process_command(&config).unwrap(), "Confirmed");

        let bob_pubkey = Pubkey::new_rand();
        config.command = WalletCommand::AuthorizeVoter(bob_pubkey);
        let signature = process_command(&config);
        assert_eq!(signature.unwrap(), SIGNATURE.to_string());

        let node_id = Pubkey::new_rand();
        config.command = WalletCommand::CreateVoteAccount(bob_pubkey, node_id, 0, 10);
        let signature = process_command(&config);
        assert_eq!(signature.unwrap(), SIGNATURE.to_string());

        config.command = WalletCommand::GetTransactionCount;
        assert_eq!(process_command(&config).unwrap(), "1234");

        config.command = WalletCommand::Pay(10, bob_pubkey, None, None, None, None);
        let signature = process_command(&config);
        assert_eq!(signature.unwrap(), SIGNATURE.to_string());

        let date_string = "\"2018-09-19T17:30:59Z\"";
        let dt: DateTime<Utc> = serde_json::from_str(&date_string).unwrap();
        config.command = WalletCommand::Pay(
            10,
            bob_pubkey,
            Some(dt),
            Some(config.keypair.pubkey()),
            None,
            None,
        );
        let result = process_command(&config);
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

        let witness = Pubkey::new_rand();
        config.command = WalletCommand::Pay(
            10,
            bob_pubkey,
            None,
            None,
            Some(vec![witness]),
            Some(config.keypair.pubkey()),
        );
        let result = process_command(&config);
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

        let process_id = Pubkey::new_rand();
        config.command = WalletCommand::TimeElapsed(bob_pubkey, process_id, dt);
        let signature = process_command(&config);
        assert_eq!(signature.unwrap(), SIGNATURE.to_string());

        let witness = Pubkey::new_rand();
        config.command = WalletCommand::Witness(bob_pubkey, witness);
        let signature = process_command(&config);
        assert_eq!(signature.unwrap(), SIGNATURE.to_string());

        // Need airdrop cases
        config.command = WalletCommand::Airdrop(50);
        assert!(process_command(&config).is_err());

        config.rpc_client = Some(RpcClient::new_mock("airdrop".to_string()));
        config.command = WalletCommand::TimeElapsed(bob_pubkey, process_id, dt);
        let signature = process_command(&config);
        assert_eq!(signature.unwrap(), SIGNATURE.to_string());

        let witness = Pubkey::new_rand();
        config.command = WalletCommand::Witness(bob_pubkey, witness);
        let signature = process_command(&config);
        assert_eq!(signature.unwrap(), SIGNATURE.to_string());

        // sig_not_found case
        config.rpc_client = Some(RpcClient::new_mock("sig_not_found".to_string()));
        let missing_signature = Signature::new(&bs58::decode("5VERv8NMvzbJMEkV8xnrLkEaWRtSz9CosKDYjCJjBRnbJLgp8uirBgmQpjKhoR4tjF3ZpRzrFmBV6UjKdiSZkQUW").into_vec().unwrap());
        config.command = WalletCommand::Confirm(missing_signature);
        assert_eq!(process_command(&config).unwrap(), "Not found");

        // Tx error case
        config.rpc_client = Some(RpcClient::new_mock("account_in_use".to_string()));
        let any_signature = Signature::new(&bs58::decode(SIGNATURE).into_vec().unwrap());
        config.command = WalletCommand::Confirm(any_signature);
        assert_eq!(
            process_command(&config).unwrap(),
            format!(
                "Transaction failed with error {:?}",
                TransactionError::AccountInUse
            )
        );

        // Failure cases
        config.rpc_client = Some(RpcClient::new_mock("fails".to_string()));

        config.command = WalletCommand::Airdrop(50);
        assert!(process_command(&config).is_err());

        config.command = WalletCommand::Balance(config.keypair.pubkey());
        assert!(process_command(&config).is_err());

        config.command = WalletCommand::AuthorizeVoter(bob_pubkey);
        assert!(process_command(&config).is_err());

        config.command = WalletCommand::CreateVoteAccount(bob_pubkey, node_id, 0, 10);
        assert!(process_command(&config).is_err());

        config.command = WalletCommand::GetTransactionCount;
        assert!(process_command(&config).is_err());

        config.command = WalletCommand::Pay(10, bob_pubkey, None, None, None, None);
        assert!(process_command(&config).is_err());

        config.command = WalletCommand::Pay(
            10,
            bob_pubkey,
            Some(dt),
            Some(config.keypair.pubkey()),
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
            Some(config.keypair.pubkey()),
        );
        assert!(process_command(&config).is_err());

        config.command = WalletCommand::TimeElapsed(bob_pubkey, process_id, dt);
        assert!(process_command(&config).is_err());
    }

    #[test]
    fn test_wallet_deploy() {
        solana_logger::setup();
        let mut pathbuf = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        pathbuf.push("tests");
        pathbuf.push("fixtures");
        pathbuf.push("noop");
        pathbuf.set_extension("so");

        // Success case
        let mut config = WalletConfig::default();
        config.rpc_client = Some(RpcClient::new_mock("succeeds".to_string()));

        config.command = WalletCommand::Deploy(pathbuf.to_str().unwrap().to_string());
        let result = process_command(&config);
        let json: Value = serde_json::from_str(&result.unwrap()).unwrap();
        let program_id = json
            .as_object()
            .unwrap()
            .get("programId")
            .unwrap()
            .as_str()
            .unwrap();
        let program_id_vec = bs58::decode(program_id).into_vec().unwrap();
        assert_eq!(program_id_vec.len(), mem::size_of::<Pubkey>());

        // Failure cases
        config.rpc_client = Some(RpcClient::new_mock("airdrop".to_string()));
        assert!(process_command(&config).is_err());

        config.command = WalletCommand::Deploy("bad/file/location.so".to_string());
        assert!(process_command(&config).is_err());
    }
}
