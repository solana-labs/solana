use chrono::prelude::*;
use clap::{App, AppSettings, Arg, ArgMatches, SubCommand};
use log::*;
use num_traits::FromPrimitive;
use serde_json;
use serde_json::json;
use solana_budget_api;
use solana_budget_api::budget_instruction;
use solana_budget_api::budget_state::BudgetError;
use solana_client::client_error::ClientError;
use solana_client::rpc_client::RpcClient;
#[cfg(not(test))]
use solana_drone::drone::request_airdrop_transaction;
use solana_drone::drone::DRONE_PORT;
#[cfg(test)]
use solana_drone::drone_mock::request_airdrop_transaction;
use solana_sdk::account_utils::State;
use solana_sdk::bpf_loader;
use solana_sdk::hash::Hash;
use solana_sdk::instruction::InstructionError;
use solana_sdk::instruction_processor_utils::DecodeError;
use solana_sdk::loader_instruction;
use solana_sdk::message::Message;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{read_keypair, Keypair, KeypairUtil, Signature};
use solana_sdk::system_instruction::SystemError;
use solana_sdk::system_transaction;
use solana_sdk::transaction::{Transaction, TransactionError};
use solana_stake_api::stake_instruction;
use solana_storage_api::storage_instruction;
use solana_vote_api::vote_instruction;
use std::fs::File;
use std::io::Read;
use std::net::{IpAddr, SocketAddr};
use std::thread::sleep;
use std::time::Duration;
use std::{error, fmt};

const USERDATA_CHUNK_SIZE: usize = 229; // Keep program chunks under PACKET_DATA_SIZE

#[derive(Debug, PartialEq)]
#[allow(clippy::large_enum_variant)]
pub enum WalletCommand {
    Address,
    Fees,
    Airdrop(u64),
    Balance(Pubkey),
    Cancel(Pubkey),
    Confirm(Signature),
    AuthorizeVoter(Pubkey, Keypair, Pubkey),
    CreateVoteAccount(Pubkey, Pubkey, u8, u64),
    ShowVoteAccount(Pubkey),
    CreateStakeAccount(Pubkey, u64),
    DelegateStake(Keypair, Pubkey, u64),
    WithdrawStake(Keypair, Pubkey, u64),
    DeactivateStake(Keypair),
    RedeemVoteCredits(Pubkey, Pubkey),
    ShowStakeAccount(Pubkey),
    CreateReplicatorStorageAccount(Pubkey, Pubkey),
    CreateValidatorStorageAccount(Pubkey, Pubkey),
    ClaimStorageReward(Pubkey, Pubkey),
    ShowStorageAccount(Pubkey),
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
    pub command: WalletCommand,
    pub drone_host: Option<IpAddr>,
    pub drone_port: u16,
    pub json_rpc_url: String,
    pub keypair: Keypair,
    pub rpc_client: Option<RpcClient>,
}

impl Default for WalletConfig {
    fn default() -> WalletConfig {
        WalletConfig {
            command: WalletCommand::Balance(Pubkey::default()),
            drone_host: None,
            drone_port: DRONE_PORT,
            json_rpc_url: "http://testnet.solana.com:8899".to_string(),
            keypair: Keypair::new(),
            rpc_client: None,
        }
    }
}

impl WalletConfig {
    pub fn drone_addr(&self) -> SocketAddr {
        SocketAddr::new(
            self.drone_host.unwrap_or_else(|| {
                let drone_host = url::Url::parse(&self.json_rpc_url)
                    .unwrap()
                    .host()
                    .unwrap()
                    .to_string();
                solana_netutil::parse_host(&drone_host).unwrap_or_else(|err| {
                    panic!("Unable to resolve {}: {}", drone_host, err);
                })
            }),
            self.drone_port,
        )
    }
}

// Return parsed values from matches at `name`
fn values_of<T>(matches: &ArgMatches<'_>, name: &str) -> Option<Vec<T>>
where
    T: std::str::FromStr,
    <T as std::str::FromStr>::Err: std::fmt::Debug,
{
    matches
        .values_of(name)
        .map(|xs| xs.map(|x| x.parse::<T>().unwrap()).collect())
}

// Return a parsed value from matches at `name`
fn value_of<T>(matches: &ArgMatches<'_>, name: &str) -> Option<T>
where
    T: std::str::FromStr,
    <T as std::str::FromStr>::Err: std::fmt::Debug,
{
    matches
        .value_of(name)
        .map(|value| value.parse::<T>().unwrap())
}

// Return the keypair for an argument with filename `name` or None if not present.
fn keypair_of(matches: &ArgMatches<'_>, name: &str) -> Option<Keypair> {
    matches.value_of(name).map(|x| read_keypair(x).unwrap())
}

pub fn parse_command(
    pubkey: &Pubkey,
    matches: &ArgMatches<'_>,
) -> Result<WalletCommand, Box<dyn error::Error>> {
    let response = match matches.subcommand() {
        ("address", Some(_address_matches)) => Ok(WalletCommand::Address),
        ("fees", Some(_fees_matches)) => Ok(WalletCommand::Fees),
        ("airdrop", Some(airdrop_matches)) => {
            let lamports = airdrop_matches.value_of("lamports").unwrap().parse()?;
            Ok(WalletCommand::Airdrop(lamports))
        }
        ("balance", Some(balance_matches)) => {
            let pubkey = value_of(&balance_matches, "pubkey").unwrap_or(*pubkey);
            Ok(WalletCommand::Balance(pubkey))
        }
        ("cancel", Some(cancel_matches)) => {
            let process_id = value_of(cancel_matches, "process_id").unwrap();
            Ok(WalletCommand::Cancel(process_id))
        }
        ("confirm", Some(confirm_matches)) => {
            match confirm_matches.value_of("signature").unwrap().parse() {
                Ok(signature) => Ok(WalletCommand::Confirm(signature)),
                _ => {
                    eprintln!("{}", confirm_matches.usage());
                    Err(WalletError::BadParameter("Invalid signature".to_string()))
                }
            }
        }
        ("create-vote-account", Some(matches)) => {
            let voting_account_pubkey = value_of(matches, "voting_account_pubkey").unwrap();
            let node_pubkey = value_of(matches, "node_pubkey").unwrap();
            let commission = if let Some(commission) = matches.value_of("commission") {
                commission.parse()?
            } else {
                0
            };
            let lamports = matches.value_of("lamports").unwrap().parse()?;
            Ok(WalletCommand::CreateVoteAccount(
                voting_account_pubkey,
                node_pubkey,
                commission,
                lamports,
            ))
        }
        ("authorize-voter", Some(matches)) => {
            let voting_account_pubkey = value_of(matches, "voting_account_pubkey").unwrap();
            let authorized_voter_keypair =
                keypair_of(matches, "authorized_voter_keypair_file").unwrap();
            let new_authorized_voter_pubkey =
                value_of(matches, "new_authorized_voter_pubkey").unwrap();

            Ok(WalletCommand::AuthorizeVoter(
                voting_account_pubkey,
                authorized_voter_keypair,
                new_authorized_voter_pubkey,
            ))
        }
        ("show-vote-account", Some(matches)) => {
            let voting_account_pubkey = value_of(matches, "voting_account_pubkey").unwrap();
            Ok(WalletCommand::ShowVoteAccount(voting_account_pubkey))
        }
        ("create-stake-account", Some(matches)) => {
            let staking_account_pubkey = value_of(matches, "staking_account_pubkey").unwrap();
            let lamports = matches.value_of("lamports").unwrap().parse()?;
            Ok(WalletCommand::CreateStakeAccount(
                staking_account_pubkey,
                lamports,
            ))
        }
        ("delegate-stake", Some(matches)) => {
            let staking_account_keypair =
                keypair_of(matches, "staking_account_keypair_file").unwrap();
            let voting_account_pubkey = value_of(matches, "voting_account_pubkey").unwrap();
            let stake = matches.value_of("stake").unwrap().parse()?;
            Ok(WalletCommand::DelegateStake(
                staking_account_keypair,
                voting_account_pubkey,
                stake,
            ))
        }
        ("withdraw-stake", Some(matches)) => {
            let staking_account_keypair =
                keypair_of(matches, "staking_account_keypair_file").unwrap();
            let destination_account_pubkey =
                value_of(matches, "destination_account_pubkey").unwrap();
            let lamports = matches.value_of("lamports").unwrap().parse()?;
            Ok(WalletCommand::WithdrawStake(
                staking_account_keypair,
                destination_account_pubkey,
                lamports,
            ))
        }
        ("deactivate-stake", Some(matches)) => {
            let staking_account_keypair =
                keypair_of(matches, "staking_account_keypair_file").unwrap();
            Ok(WalletCommand::DeactivateStake(staking_account_keypair))
        }
        ("redeem-vote-credits", Some(matches)) => {
            let staking_account_pubkey = value_of(matches, "staking_account_pubkey").unwrap();
            let voting_account_pubkey = value_of(matches, "voting_account_pubkey").unwrap();
            Ok(WalletCommand::RedeemVoteCredits(
                staking_account_pubkey,
                voting_account_pubkey,
            ))
        }
        ("show-stake-account", Some(matches)) => {
            let staking_account_pubkey = value_of(matches, "staking_account_pubkey").unwrap();
            Ok(WalletCommand::ShowStakeAccount(staking_account_pubkey))
        }
        ("create-replicator-storage-account", Some(matches)) => {
            let account_owner = value_of(matches, "storage_account_owner").unwrap();
            let storage_account_pubkey = value_of(matches, "storage_account_pubkey").unwrap();
            Ok(WalletCommand::CreateReplicatorStorageAccount(
                account_owner,
                storage_account_pubkey,
            ))
        }
        ("create-validator-storage-account", Some(matches)) => {
            let account_owner = value_of(matches, "storage_account_owner").unwrap();
            let storage_account_pubkey = value_of(matches, "storage_account_pubkey").unwrap();
            Ok(WalletCommand::CreateValidatorStorageAccount(
                account_owner,
                storage_account_pubkey,
            ))
        }
        ("claim-storage-reward", Some(matches)) => {
            let node_account_pubkey = value_of(matches, "node_account_pubkey").unwrap();
            let storage_account_pubkey = value_of(matches, "storage_account_pubkey").unwrap();
            Ok(WalletCommand::ClaimStorageReward(
                node_account_pubkey,
                storage_account_pubkey,
            ))
        }
        ("show-storage-account", Some(matches)) => {
            let storage_account_pubkey = value_of(matches, "storage_account_pubkey").unwrap();
            Ok(WalletCommand::ShowStorageAccount(storage_account_pubkey))
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
            let to = value_of(&pay_matches, "to").unwrap_or(*pubkey);
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
            let timestamp_pubkey = value_of(&pay_matches, "timestamp_pubkey");
            let witness_vec = values_of(&pay_matches, "witness");
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
            let to = value_of(&sig_matches, "to").unwrap();
            let process_id = value_of(&sig_matches, "process_id").unwrap();
            Ok(WalletCommand::Witness(to, process_id))
        }
        ("send-timestamp", Some(timestamp_matches)) => {
            let to = value_of(&timestamp_matches, "to").unwrap();
            let process_id = value_of(&timestamp_matches, "process_id").unwrap();
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

fn process_fees(rpc_client: &RpcClient) -> ProcessResult {
    let (recent_blockhash, fee_calculator) = rpc_client.get_recent_blockhash()?;

    Ok(format!(
        "blockhash: {}\nlamports per signature: {}",
        recent_blockhash, fee_calculator.lamports_per_signature
    ))
}
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

fn process_confirm(rpc_client: &RpcClient, signature: &Signature) -> ProcessResult {
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

fn process_create_vote_account(
    rpc_client: &RpcClient,
    config: &WalletConfig,
    voting_account_pubkey: &Pubkey,
    node_pubkey: &Pubkey,
    commission: u8,
    lamports: u64,
) -> ProcessResult {
    let ixs = vote_instruction::create_account(
        &config.keypair.pubkey(),
        voting_account_pubkey,
        node_pubkey,
        commission,
        lamports,
    );
    let (recent_blockhash, _fee_calculator) = rpc_client.get_recent_blockhash()?;
    let mut tx = Transaction::new_signed_instructions(&[&config.keypair], ixs, recent_blockhash);
    let signature_str = rpc_client.send_and_confirm_transaction(&mut tx, &[&config.keypair])?;
    Ok(signature_str.to_string())
}

fn process_authorize_voter(
    rpc_client: &RpcClient,
    config: &WalletConfig,
    voting_account_pubkey: &Pubkey,
    authorized_voter_keypair: &Keypair,
    new_authorized_voter_pubkey: &Pubkey,
) -> ProcessResult {
    let (recent_blockhash, _fee_calculator) = rpc_client.get_recent_blockhash()?;
    let ixs = vec![vote_instruction::authorize_voter(
        voting_account_pubkey,              // vote account to update
        &authorized_voter_keypair.pubkey(), // current authorized voter (often the vote account itself)
        new_authorized_voter_pubkey,        // new vote signer
    )];

    let mut tx = Transaction::new_signed_with_payer(
        ixs,
        Some(&config.keypair.pubkey()),
        &[&config.keypair, &authorized_voter_keypair],
        recent_blockhash,
    );
    let signature_str = rpc_client
        .send_and_confirm_transaction(&mut tx, &[&config.keypair, &authorized_voter_keypair])?;
    Ok(signature_str.to_string())
}

fn process_show_vote_account(
    rpc_client: &RpcClient,
    _config: &WalletConfig,
    voting_account_pubkey: &Pubkey,
) -> ProcessResult {
    use solana_vote_api::vote_state::VoteState;
    let vote_account_lamports = rpc_client.retry_get_balance(voting_account_pubkey, 5)?;
    let vote_account_data = rpc_client.get_account_data(voting_account_pubkey)?;
    let vote_state = VoteState::deserialize(&vote_account_data).map_err(|_| {
        WalletError::RpcRequestError(
            "Account data could not be deserialized to vote state".to_string(),
        )
    })?;

    println!("account lamports: {}", vote_account_lamports.unwrap());
    println!("node id: {}", vote_state.node_pubkey);
    println!(
        "authorized voter pubkey: {}",
        vote_state.authorized_voter_pubkey
    );
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
        println!("recent votes:");
        for vote in &vote_state.votes {
            println!(
                "- slot: {}\n  confirmation count: {}",
                vote.slot, vote.confirmation_count
            );
        }

        // TODO: Use the real GenesisBlock from the cluster.
        let genesis_block = solana_sdk::genesis_block::GenesisBlock::default();
        let epoch_schedule = solana_runtime::epoch_schedule::EpochSchedule::new(
            genesis_block.slots_per_epoch,
            genesis_block.stakers_slot_offset,
            genesis_block.epoch_warmup,
        );

        println!("epoch voting history:");
        for (epoch, credits, prev_credits) in vote_state.epoch_credits() {
            let credits_earned = credits - prev_credits;
            let slots_in_epoch = epoch_schedule.get_slots_in_epoch(*epoch);
            println!(
                "- epoch: {}\n  slots in epoch: {}\n  credits earned: {}",
                epoch, slots_in_epoch, credits_earned,
            );
        }
    }
    Ok("".to_string())
}

fn process_create_stake_account(
    rpc_client: &RpcClient,
    config: &WalletConfig,
    staking_account_pubkey: &Pubkey,
    lamports: u64,
) -> ProcessResult {
    let (recent_blockhash, _fee_calculator) = rpc_client.get_recent_blockhash()?;
    let ixs = stake_instruction::create_stake_account(
        &config.keypair.pubkey(),
        staking_account_pubkey,
        lamports,
    );
    let mut tx = Transaction::new_signed_with_payer(
        ixs,
        Some(&config.keypair.pubkey()),
        &[&config.keypair],
        recent_blockhash,
    );
    let signature_str = rpc_client.send_and_confirm_transaction(&mut tx, &[&config.keypair])?;
    Ok(signature_str.to_string())
}

fn process_deactivate_stake_account(
    rpc_client: &RpcClient,
    config: &WalletConfig,
    staking_account_keypair: &Keypair,
) -> ProcessResult {
    let (recent_blockhash, _fee_calculator) = rpc_client.get_recent_blockhash()?;
    let ixs = stake_instruction::deactivate_stake(&staking_account_keypair.pubkey());
    let mut tx = Transaction::new_signed_with_payer(
        vec![ixs],
        Some(&config.keypair.pubkey()),
        &[&config.keypair, &staking_account_keypair],
        recent_blockhash,
    );
    let signature_str = rpc_client
        .send_and_confirm_transaction(&mut tx, &[&config.keypair, &staking_account_keypair])?;
    Ok(signature_str.to_string())
}

fn process_delegate_stake(
    rpc_client: &RpcClient,
    config: &WalletConfig,
    staking_account_keypair: &Keypair,
    voting_account_pubkey: &Pubkey,
    stake: u64,
) -> ProcessResult {
    let (recent_blockhash, _fee_calculator) = rpc_client.get_recent_blockhash()?;
    let ixs = vec![stake_instruction::delegate_stake(
        &staking_account_keypair.pubkey(),
        voting_account_pubkey,
        stake,
    )];

    let mut tx = Transaction::new_signed_with_payer(
        ixs,
        Some(&config.keypair.pubkey()),
        &[&config.keypair, &staking_account_keypair],
        recent_blockhash,
    );

    let signature_str = rpc_client
        .send_and_confirm_transaction(&mut tx, &[&config.keypair, &staking_account_keypair])?;
    Ok(signature_str.to_string())
}

fn process_withdraw_stake(
    rpc_client: &RpcClient,
    config: &WalletConfig,
    staking_account_keypair: &Keypair,
    destination_account_pubkey: &Pubkey,
    lamports: u64,
) -> ProcessResult {
    let (recent_blockhash, _fee_calculator) = rpc_client.get_recent_blockhash()?;
    let ixs = vec![stake_instruction::withdraw(
        &staking_account_keypair.pubkey(),
        destination_account_pubkey,
        lamports,
    )];

    let mut tx = Transaction::new_signed_with_payer(
        ixs,
        Some(&config.keypair.pubkey()),
        &[&config.keypair, &staking_account_keypair],
        recent_blockhash,
    );

    let signature_str = rpc_client
        .send_and_confirm_transaction(&mut tx, &[&config.keypair, &staking_account_keypair])?;
    Ok(signature_str.to_string())
}

fn process_redeem_vote_credits(
    rpc_client: &RpcClient,
    config: &WalletConfig,
    staking_account_pubkey: &Pubkey,
    voting_account_pubkey: &Pubkey,
) -> ProcessResult {
    let (recent_blockhash, _fee_calculator) = rpc_client.get_recent_blockhash()?;
    let ixs = vec![stake_instruction::redeem_vote_credits(
        staking_account_pubkey,
        voting_account_pubkey,
    )];
    let mut tx = Transaction::new_signed_with_payer(
        ixs,
        Some(&config.keypair.pubkey()),
        &[&config.keypair],
        recent_blockhash,
    );
    let signature_str = rpc_client.send_and_confirm_transaction(&mut tx, &[&config.keypair])?;
    Ok(signature_str.to_string())
}

fn process_show_stake_account(
    rpc_client: &RpcClient,
    _config: &WalletConfig,
    staking_account_pubkey: &Pubkey,
) -> ProcessResult {
    use solana_stake_api::stake_state::StakeState;
    let stake_account = rpc_client.get_account(staking_account_pubkey)?;
    match stake_account.state() {
        Ok(StakeState::Stake(stake)) => {
            println!("account lamports: {}", stake_account.lamports);
            println!("voter pubkey: {}", stake.voter_pubkey);
            println!("credits observed: {}", stake.credits_observed);
            println!("stake: {}", stake.stake);
            Ok("".to_string())
        }
        _ => Err(WalletError::RpcRequestError(
            "Account data could not be deserialized to stake state".to_string(),
        ))?,
    }
}

fn process_create_replicator_storage_account(
    rpc_client: &RpcClient,
    config: &WalletConfig,
    account_owner: &Pubkey,
    storage_account_pubkey: &Pubkey,
) -> ProcessResult {
    let (recent_blockhash, _fee_calculator) = rpc_client.get_recent_blockhash()?;
    let ixs = storage_instruction::create_replicator_storage_account(
        &config.keypair.pubkey(),
        &account_owner,
        storage_account_pubkey,
        1,
    );
    let mut tx = Transaction::new_signed_instructions(&[&config.keypair], ixs, recent_blockhash);
    let signature_str = rpc_client.send_and_confirm_transaction(&mut tx, &[&config.keypair])?;
    Ok(signature_str.to_string())
}

fn process_create_validator_storage_account(
    rpc_client: &RpcClient,
    config: &WalletConfig,
    account_owner: &Pubkey,
    storage_account_pubkey: &Pubkey,
) -> ProcessResult {
    let (recent_blockhash, _fee_calculator) = rpc_client.get_recent_blockhash()?;
    let ixs = storage_instruction::create_validator_storage_account(
        &config.keypair.pubkey(),
        account_owner,
        storage_account_pubkey,
        1,
    );
    let mut tx = Transaction::new_signed_instructions(&[&config.keypair], ixs, recent_blockhash);
    let signature_str = rpc_client.send_and_confirm_transaction(&mut tx, &[&config.keypair])?;
    Ok(signature_str.to_string())
}

fn process_claim_storage_reward(
    rpc_client: &RpcClient,
    config: &WalletConfig,
    node_account_pubkey: &Pubkey,
    storage_account_pubkey: &Pubkey,
) -> ProcessResult {
    let (recent_blockhash, _fee_calculator) = rpc_client.get_recent_blockhash()?;

    let instruction =
        storage_instruction::claim_reward(node_account_pubkey, storage_account_pubkey);
    let signers = [&config.keypair];
    let message = Message::new_with_payer(vec![instruction], Some(&signers[0].pubkey()));

    let mut transaction = Transaction::new(&signers, message, recent_blockhash);
    let signature_str = rpc_client.send_and_confirm_transaction(&mut transaction, &signers)?;
    Ok(signature_str.to_string())
}

fn process_show_storage_account(
    rpc_client: &RpcClient,
    _config: &WalletConfig,
    storage_account_pubkey: &Pubkey,
) -> ProcessResult {
    use solana_storage_api::storage_contract::StorageContract;
    let account = rpc_client.get_account(storage_account_pubkey)?;
    let storage_contract: StorageContract = account.state().map_err(|err| {
        WalletError::RpcRequestError(
            format!("Unable to deserialize storage account: {:?}", err).to_string(),
        )
    })?;
    println!("{:#?}", storage_contract);
    println!("account lamports: {}", account.lamports);
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

    let (blockhash, _fee_calculator) = rpc_client.get_recent_blockhash()?;
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
    );
    trace!("Creating program account");
    let result = rpc_client.send_and_confirm_transaction(&mut tx, &[&config.keypair]);
    log_instruction_custom_error::<SystemError>(result).map_err(|_| {
        WalletError::DynamicProgramError("Program allocate space failed".to_string())
    })?;

    trace!("Writing program data");
    let signers = [&config.keypair, &program_id];
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
            let message = Message::new_with_payer(vec![instruction], Some(&signers[0].pubkey()));
            Transaction::new(&signers, message, blockhash)
        })
        .collect();
    rpc_client.send_and_confirm_transactions(write_transactions, &signers)?;

    trace!("Finalizing program account");
    let instruction = loader_instruction::finalize(&program_id.pubkey(), &bpf_loader::id());
    let message = Message::new_with_payer(vec![instruction], Some(&signers[0].pubkey()));
    let mut tx = Transaction::new(&signers, message, blockhash);
    rpc_client
        .send_and_confirm_transaction(&mut tx, &signers)
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
    let (blockhash, _fee_calculator) = rpc_client.get_recent_blockhash()?;

    if timestamp == None && *witnesses == None {
        let mut tx = system_transaction::transfer(&config.keypair, to, lamports, blockhash);
        let result = rpc_client.send_and_confirm_transaction(&mut tx, &[&config.keypair]);
        let signature_str = log_instruction_custom_error::<SystemError>(result)?;
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
        let result = rpc_client.send_and_confirm_transaction(&mut tx, &[&config.keypair]);
        let signature_str = log_instruction_custom_error::<BudgetError>(result)?;

        Ok(json!({
            "signature": signature_str,
            "processId": format!("{}", contract_state.pubkey()),
        })
        .to_string())
    } else if timestamp == None {
        let (blockhash, _fee_calculator) = rpc_client.get_recent_blockhash()?;

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
        let result = rpc_client.send_and_confirm_transaction(&mut tx, &[&config.keypair]);
        let signature_str = log_instruction_custom_error::<BudgetError>(result)?;

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
    let (blockhash, _fee_calculator) = rpc_client.get_recent_blockhash()?;
    let ix = budget_instruction::apply_signature(
        &config.keypair.pubkey(),
        pubkey,
        &config.keypair.pubkey(),
    );
    let mut tx = Transaction::new_signed_instructions(&[&config.keypair], vec![ix], blockhash);
    let result = rpc_client.send_and_confirm_transaction(&mut tx, &[&config.keypair]);
    let signature_str = log_instruction_custom_error::<BudgetError>(result)?;
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

    let (blockhash, _fee_calculator) = rpc_client.get_recent_blockhash()?;

    let ix = budget_instruction::apply_timestamp(&config.keypair.pubkey(), pubkey, to, dt);
    let mut tx = Transaction::new_signed_instructions(&[&config.keypair], vec![ix], blockhash);
    let result = rpc_client.send_and_confirm_transaction(&mut tx, &[&config.keypair]);
    let signature_str = log_instruction_custom_error::<BudgetError>(result)?;

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

    let (blockhash, _fee_calculator) = rpc_client.get_recent_blockhash()?;
    let ix = budget_instruction::apply_signature(&config.keypair.pubkey(), pubkey, to);
    let mut tx = Transaction::new_signed_instructions(&[&config.keypair], vec![ix], blockhash);
    let result = rpc_client.send_and_confirm_transaction(&mut tx, &[&config.keypair]);
    let signature_str = log_instruction_custom_error::<BudgetError>(result)?;

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
        _rpc_client = RpcClient::new(config.json_rpc_url.to_string());
        &_rpc_client
    } else {
        // Primarily for testing
        config.rpc_client.as_ref().unwrap()
    };

    match &config.command {
        // Get address of this client
        WalletCommand::Address => unreachable!(),

        WalletCommand::Fees => process_fees(&rpc_client),

        // Request an airdrop from Solana Drone;
        WalletCommand::Airdrop(lamports) => {
            process_airdrop(&rpc_client, config, drone_addr, *lamports)
        }

        // Check client balance
        WalletCommand::Balance(pubkey) => process_balance(&pubkey, &rpc_client),

        // Cancel a contract by contract Pubkey
        WalletCommand::Cancel(pubkey) => process_cancel(&rpc_client, config, &pubkey),

        // Confirm the last client transaction by signature
        WalletCommand::Confirm(signature) => process_confirm(&rpc_client, signature),

        // Create vote account
        WalletCommand::CreateVoteAccount(
            voting_account_pubkey,
            node_pubkey,
            commission,
            lamports,
        ) => process_create_vote_account(
            &rpc_client,
            config,
            &voting_account_pubkey,
            &node_pubkey,
            *commission,
            *lamports,
        ),
        // Configure staking account already created
        WalletCommand::AuthorizeVoter(
            voting_account_pubkey,
            authorized_voter_keypair,
            new_authorized_voter_pubkey,
        ) => process_authorize_voter(
            &rpc_client,
            config,
            &voting_account_pubkey,
            &authorized_voter_keypair,
            &new_authorized_voter_pubkey,
        ),
        // Show a vote account
        WalletCommand::ShowVoteAccount(voting_account_pubkey) => {
            process_show_vote_account(&rpc_client, config, &voting_account_pubkey)
        }

        // Create stake account
        WalletCommand::CreateStakeAccount(staking_account_pubkey, lamports) => {
            process_create_stake_account(&rpc_client, config, &staking_account_pubkey, *lamports)
        }

        WalletCommand::DelegateStake(staking_account_keypair, voting_account_pubkey, lamports) => {
            process_delegate_stake(
                &rpc_client,
                config,
                &staking_account_keypair,
                &voting_account_pubkey,
                *lamports,
            )
        }

        WalletCommand::WithdrawStake(
            staking_account_keypair,
            destination_account_pubkey,
            lamports,
        ) => process_withdraw_stake(
            &rpc_client,
            config,
            &staking_account_keypair,
            &destination_account_pubkey,
            *lamports,
        ),

        // Deactivate stake account
        WalletCommand::DeactivateStake(staking_account_keypair) => {
            process_deactivate_stake_account(&rpc_client, config, &staking_account_keypair)
        }

        WalletCommand::RedeemVoteCredits(staking_account_pubkey, voting_account_pubkey) => {
            process_redeem_vote_credits(
                &rpc_client,
                config,
                &staking_account_pubkey,
                &voting_account_pubkey,
            )
        }

        WalletCommand::ShowStakeAccount(staking_account_pubkey) => {
            process_show_stake_account(&rpc_client, config, &staking_account_pubkey)
        }

        WalletCommand::CreateReplicatorStorageAccount(
            storage_account_owner,
            storage_account_pubkey,
        ) => process_create_replicator_storage_account(
            &rpc_client,
            config,
            &storage_account_owner,
            &storage_account_pubkey,
        ),

        WalletCommand::CreateValidatorStorageAccount(account_owner, storage_account_pubkey) => {
            process_create_validator_storage_account(
                &rpc_client,
                config,
                &account_owner,
                &storage_account_pubkey,
            )
        }

        WalletCommand::ClaimStorageReward(node_account_pubkey, storage_account_pubkey) => {
            process_claim_storage_reward(
                &rpc_client,
                config,
                node_account_pubkey,
                &storage_account_pubkey,
            )
        }

        WalletCommand::ShowStorageAccount(storage_account_pubkey) => {
            process_show_storage_account(&rpc_client, config, &storage_account_pubkey)
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
            *lamports,
            &to,
            *timestamp,
            *timestamp_pubkey,
            witnesses,
            *cancelable,
        ),

        // Apply time elapsed to contract
        WalletCommand::TimeElapsed(to, pubkey, dt) => {
            process_time_elapsed(&rpc_client, config, drone_addr, &to, &pubkey, *dt)
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
    let (blockhash, _fee_calculator) = rpc_client.get_recent_blockhash()?;
    let keypair = {
        let mut retries = 5;
        loop {
            let result = DroneKeypair::new_keypair(drone_addr, to_pubkey, lamports, blockhash);
            if result.is_ok() || retries == 0 {
                break result;
            }
            retries -= 1;
            sleep(Duration::from_secs(1));
        }
    }?;
    let mut tx = keypair.airdrop_transaction();
    let result = rpc_client.send_and_confirm_transaction(&mut tx, &[&keypair]);
    log_instruction_custom_error::<SystemError>(result)?;
    Ok(())
}

fn log_instruction_custom_error<E>(result: Result<String, ClientError>) -> ProcessResult
where
    E: 'static + std::error::Error + DecodeError<E> + FromPrimitive,
{
    if result.is_err() {
        let err = result.unwrap_err();
        if let ClientError::TransactionError(TransactionError::InstructionError(
            _,
            InstructionError::CustomError(code),
        )) = err
        {
            if let Some(specific_error) = E::decode_custom_error_to_enum(code) {
                error!(
                    "{:?}: {}::{:?}",
                    err,
                    specific_error.type_of(),
                    specific_error
                );
                Err(specific_error)?
            }
        }
        error!("{:?}", err);
        Err(err)?
    } else {
        Ok(result.unwrap())
    }
}

// Return an error if a pubkey cannot be parsed.
fn is_pubkey(string: String) -> Result<(), String> {
    match string.parse::<Pubkey>() {
        Ok(_) => Ok(()),
        Err(err) => Err(format!("{:?}", err)),
    }
}

pub fn app<'ab, 'v>(name: &str, about: &'ab str, version: &'v str) -> App<'ab, 'v> {
    App::new(name)
        .about(about)
        .version(version)
        .setting(AppSettings::SubcommandRequiredElseHelp)
        .subcommand(SubCommand::with_name("address").about("Get your public key"))
        .subcommand(SubCommand::with_name("fees").about("Display current cluster fees"))
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
        .subcommand(
            SubCommand::with_name("balance")
                .about("Get your balance")
                .arg(
                    Arg::with_name("pubkey")
                        .index(1)
                        .value_name("PUBKEY")
                        .takes_value(true)
                        .validator(is_pubkey)
                        .help("The public key of the balance to check"),
                ),
        )
        .subcommand(
            SubCommand::with_name("cancel")
                .about("Cancel a transfer")
                .arg(
                    Arg::with_name("process_id")
                        .index(1)
                        .value_name("PROCESS_ID")
                        .takes_value(true)
                        .required(true)
                        .validator(is_pubkey)
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
                .about("Authorize a new vote signing keypair for the given vote account")
                .arg(
                    Arg::with_name("voting_account_pubkey")
                        .index(1)
                        .value_name("PUBKEY")
                        .takes_value(true)
                        .required(true)
                        .validator(is_pubkey)
                        .help("Vote account in which to set the authorized voter"),
                )
                .arg(
                    Arg::with_name("authorized_voter_keypair_file")
                        .index(2)
                        .value_name("KEYPAIR_FILE")
                        .takes_value(true)
                        .required(true)
                        .help("Keypair file for the currently authorized vote signer"),
                )
                .arg(
                    Arg::with_name("new_authorized_voter_pubkey")
                        .index(3)
                        .value_name("PUBKEY")
                        .takes_value(true)
                        .required(true)
                        .validator(is_pubkey)
                        .help("New vote signer to authorize"),
                ),
        )
        .subcommand(
            SubCommand::with_name("create-vote-account")
                .about("Create vote account for a node")
                .arg(
                    Arg::with_name("voting_account_pubkey")
                        .index(1)
                        .value_name("PUBKEY")
                        .takes_value(true)
                        .required(true)
                        .validator(is_pubkey)
                        .help("Vote account address to fund"),
                )
                .arg(
                    Arg::with_name("node_pubkey")
                        .index(2)
                        .value_name("PUBKEY")
                        .takes_value(true)
                        .required(true)
                        .validator(is_pubkey)
                        .help("Node that will vote in this account"),
                )
                .arg(
                    Arg::with_name("lamports")
                        .index(3)
                        .value_name("NUM")
                        .takes_value(true)
                        .required(true)
                        .help("The number of lamports to send to the vote account"),
                )
                .arg(
                    Arg::with_name("commission")
                        .long("commission")
                        .value_name("NUM")
                        .takes_value(true)
                        .help("The commission taken on reward redemption (0-255), default: 0"),
                ),
        )
        .subcommand(
            SubCommand::with_name("show-vote-account")
                .about("Show the contents of a vote account")
                .arg(
                    Arg::with_name("voting_account_pubkey")
                        .index(1)
                        .value_name("PUBKEY")
                        .takes_value(true)
                        .required(true)
                        .validator(is_pubkey)
                        .help("Vote account pubkey"),
                )
        )
       .subcommand(
            SubCommand::with_name("create-stake-account")
                .about("Create staking account")
                .arg(
                    Arg::with_name("staking_account_pubkey")
                        .index(1)
                        .value_name("PUBKEY")
                        .takes_value(true)
                        .required(true)
                        .validator(is_pubkey)
                        .help("Staking account address to fund"),
                )
                .arg(
                    Arg::with_name("lamports")
                        .index(2)
                        .value_name("NUM")
                        .takes_value(true)
                        .required(true)
                        .help("The number of lamports to send to staking account"),
                ),
        )
        .subcommand(
            SubCommand::with_name("delegate-stake")
                .about("Delegate the stake to some vote account")
                .arg(
                    Arg::with_name("staking_account_keypair_file")
                        .index(1)
                        .value_name("KEYPAIR_FILE")
                        .takes_value(true)
                        .required(true)
                        .help("Keypair file for the staking account, for signing the delegate transaction."),
                )
                .arg(
                    Arg::with_name("voting_account_pubkey")
                        .index(2)
                        .value_name("PUBKEY")
                        .takes_value(true)
                        .required(true)
                        .validator(is_pubkey)
                        .help("The voting account to which the stake will be delegated"),
                )
                .arg(
                    Arg::with_name("stake")
                        .index(3)
                        .value_name("NUM")
                        .takes_value(true)
                        .required(true)
                        .help("The number of lamports to stake, must be less than the stake account's balance."),
                ),
        )
        .subcommand(
            SubCommand::with_name("deactivate-stake")
                .about("Deactivate the delegated stake from the staking account")
                .arg(
                    Arg::with_name("staking_account_keypair_file")
                        .index(1)
                        .value_name("KEYPAIR_FILE")
                        .takes_value(true)
                        .required(true)
                        .help("Keypair file for the staking account, for signing the delegate transaction."),
                )
        )
        .subcommand(
            SubCommand::with_name("withdraw-stake")
                .about("Withdraw the unstaked lamports from the stake account")
                .arg(
                    Arg::with_name("staking_account_keypair_file")
                        .index(1)
                        .value_name("KEYPAIR_FILE")
                        .takes_value(true)
                        .required(true)
                        .help("Keypair file for the staking account, for signing the withdraw transaction."),
                )
                .arg(
                    Arg::with_name("destination_account_pubkey")
                        .index(2)
                        .value_name("PUBKEY")
                        .takes_value(true)
                        .required(true)
                        .validator(is_pubkey)
                        .help("The account where the lamports should be transfered"),
                )
                .arg(
                    Arg::with_name("lamports")
                        .index(3)
                        .value_name("NUM")
                        .takes_value(true)
                        .required(true)
                        .help("The number of lamports to to withdraw from the stake account."),
                ),
        )
        .subcommand(
            SubCommand::with_name("redeem-vote-credits")
                .about("Redeem credits in the staking account")
                .arg(
                    Arg::with_name("mining_pool_account_pubkey")
                        .index(1)
                        .value_name("MINING POOL PUBKEY")
                        .takes_value(true)
                        .required(true)
                        .validator(is_pubkey)
                        .help("Mining pool account to redeem credits from"),
                )
                .arg(
                    Arg::with_name("staking_account_pubkey")
                        .index(2)
                        .value_name("STAKING ACCOUNT PUBKEY")
                        .takes_value(true)
                        .required(true)
                        .validator(is_pubkey)
                        .help("Staking account address to redeem credits for"),
                )
                .arg(
                    Arg::with_name("voting_account_pubkey")
                        .index(3)
                        .value_name("PUBKEY")
                        .takes_value(true)
                        .required(true)
                        .validator(is_pubkey)
                        .help("The voting account to which the stake was previously delegated."),
                ),
        )
        .subcommand(
            SubCommand::with_name("show-stake-account")
                .about("Show the contents of a stake account")
                .arg(
                    Arg::with_name("staking_account_pubkey")
                        .index(1)
                        .value_name("PUBKEY")
                        .takes_value(true)
                        .required(true)
                        .validator(is_pubkey)
                        .help("Stake account pubkey"),
                )
        )
        .subcommand(
            SubCommand::with_name("create-storage-mining-pool-account")
                .about("Create mining pool account")
                .arg(
                    Arg::with_name("storage_account_pubkey")
                        .index(1)
                        .value_name("PUBKEY")
                        .takes_value(true)
                        .required(true)
                        .validator(is_pubkey)
                        .help("Storage mining pool account address to fund"),
                )
                .arg(
                    Arg::with_name("lamports")
                        .index(2)
                        .value_name("NUM")
                        .takes_value(true)
                        .required(true)
                        .help("The number of lamports to assign to the storage mining pool account"),
                ),
        )
        .subcommand(
            SubCommand::with_name("create-replicator-storage-account")
                .about("Create a replicator storage account")
                .arg(
                    Arg::with_name("storage_account_owner")
                        .index(1)
                        .value_name("PUBKEY")
                        .takes_value(true)
                        .required(true)
                        .validator(is_pubkey)
                )
                .arg(
                    Arg::with_name("storage_account_pubkey")
                        .index(2)
                        .value_name("PUBKEY")
                        .takes_value(true)
                        .required(true)
                        .validator(is_pubkey)
                )
        )
        .subcommand(
            SubCommand::with_name("create-validator-storage-account")
                .about("Create a validator storage account")
                .arg(
                    Arg::with_name("storage_account_owner")
                        .index(1)
                        .value_name("PUBKEY")
                        .takes_value(true)
                        .required(true)
                        .validator(is_pubkey)
                )
                .arg(
                    Arg::with_name("storage_account_pubkey")
                        .index(2)
                        .value_name("PUBKEY")
                        .takes_value(true)
                        .required(true)
                        .validator(is_pubkey)
                )
        )
        .subcommand(
            SubCommand::with_name("claim-storage-reward")
                .about("Redeem storage reward credits")
                .arg(
                    Arg::with_name("node_account_pubkey")
                        .index(1)
                        .value_name("NODE PUBKEY")
                        .takes_value(true)
                        .required(true)
                        .validator(is_pubkey)
                        .help("The node account to credit the rewards to"),
                )
                .arg(
                    Arg::with_name("storage_account_pubkey")
                        .index(3)
                        .value_name("STORAGE ACCOUNT PUBKEY")
                        .takes_value(true)
                        .required(true)
                        .validator(is_pubkey)
                        .help("Storage account address to redeem credits for"),
                ))

        .subcommand(
            SubCommand::with_name("show-storage-account")
                .about("Show the contents of a storage account")
                .arg(
                    Arg::with_name("storage_account_pubkey")
                        .index(1)
                        .value_name("PUBKEY")
                        .takes_value(true)
                        .required(true)
                        .validator(is_pubkey)
                        .help("Storage account pubkey"),
                )
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
                        .validator(is_pubkey)
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
                        .validator(is_pubkey)
                        .help("Require timestamp from this third party"),
                )
                .arg(
                    Arg::with_name("witness")
                        .long("require-signature-from")
                        .value_name("PUBKEY")
                        .takes_value(true)
                        .multiple(true)
                        .use_delimiter(true)
                        .validator(is_pubkey)
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
                        .validator(is_pubkey)
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
                        .validator(is_pubkey)
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
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use solana_client::mock_rpc_client_request::SIGNATURE;
    use solana_sdk::signature::gen_keypair_file;
    use solana_sdk::transaction::TransactionError;
    use std::net::{Ipv4Addr, SocketAddr};
    use std::path::PathBuf;

    #[test]
    fn test_wallet_config_drone_addr() {
        let mut config = WalletConfig::default();
        config.json_rpc_url = "http://127.0.0.1:8899".to_string();
        let rpc_host = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1));
        assert_eq!(
            config.drone_addr(),
            SocketAddr::new(rpc_host, config.drone_port)
        );

        config.drone_port = 1234;
        assert_eq!(config.drone_addr(), SocketAddr::new(rpc_host, 1234));

        config.drone_host = Some(rpc_host);
        assert_eq!(
            config.drone_addr(),
            SocketAddr::new(config.drone_host.unwrap(), 1234)
        );
    }

    #[test]
    fn test_wallet_parse_command() {
        let test_commands = app("test", "desc", "version");

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
        let keypair_file = make_tmp_path("keypair_file");
        gen_keypair_file(&keypair_file).unwrap();
        let keypair = read_keypair(&keypair_file).unwrap();

        let test_authorize_voter = test_commands.clone().get_matches_from(vec![
            "test",
            "authorize-voter",
            &pubkey_string,
            &keypair_file,
            &pubkey_string,
        ]);
        assert_eq!(
            parse_command(&pubkey, &test_authorize_voter).unwrap(),
            WalletCommand::AuthorizeVoter(pubkey, keypair, pubkey)
        );

        // Test CreateVoteAccount SubCommand
        let node_pubkey = Pubkey::new_rand();
        let node_pubkey_string = format!("{}", node_pubkey);
        let test_create_vote_account = test_commands.clone().get_matches_from(vec![
            "test",
            "create-vote-account",
            &pubkey_string,
            &node_pubkey_string,
            "50",
            "--commission",
            "10",
        ]);
        assert_eq!(
            parse_command(&pubkey, &test_create_vote_account).unwrap(),
            WalletCommand::CreateVoteAccount(pubkey, node_pubkey, 10, 50)
        );
        let test_create_vote_account2 = test_commands.clone().get_matches_from(vec![
            "test",
            "create-vote-account",
            &pubkey_string,
            &node_pubkey_string,
            "50",
        ]);
        assert_eq!(
            parse_command(&pubkey, &test_create_vote_account2).unwrap(),
            WalletCommand::CreateVoteAccount(pubkey, node_pubkey, 0, 50)
        );

        // Test Create Stake Account
        let test_create_stake_account = test_commands.clone().get_matches_from(vec![
            "test",
            "create-stake-account",
            &pubkey_string,
            "50",
        ]);
        assert_eq!(
            parse_command(&pubkey, &test_create_stake_account).unwrap(),
            WalletCommand::CreateStakeAccount(pubkey, 50)
        );

        fn make_tmp_path(name: &str) -> String {
            let out_dir = std::env::var("OUT_DIR").unwrap_or_else(|_| "farf".to_string());
            let keypair = Keypair::new();

            let path = format!("{}/tmp/{}-{}", out_dir, name, keypair.pubkey());

            // whack any possible collision
            let _ignored = std::fs::remove_dir_all(&path);
            // whack any possible collision
            let _ignored = std::fs::remove_file(&path);

            path
        }

        let keypair_file = make_tmp_path("keypair_file");
        gen_keypair_file(&keypair_file).unwrap();
        let keypair = read_keypair(&keypair_file).unwrap();
        // Test Delegate Stake Subcommand
        let test_delegate_stake = test_commands.clone().get_matches_from(vec![
            "test",
            "delegate-stake",
            &keypair_file,
            &pubkey_string,
            "42",
        ]);
        assert_eq!(
            parse_command(&pubkey, &test_delegate_stake).unwrap(),
            WalletCommand::DelegateStake(keypair, pubkey, 42)
        );

        let keypair_file = make_tmp_path("keypair_file");
        gen_keypair_file(&keypair_file).unwrap();
        let keypair = read_keypair(&keypair_file).unwrap();
        // Test Withdraw from Stake Account
        let test_withdraw_stake = test_commands.clone().get_matches_from(vec![
            "test",
            "withdraw-stake",
            &keypair_file,
            &pubkey_string,
            "42",
        ]);
        assert_eq!(
            parse_command(&pubkey, &test_withdraw_stake).unwrap(),
            WalletCommand::WithdrawStake(keypair, pubkey, 42)
        );

        // Test Deactivate Stake Subcommand
        let keypair_file = make_tmp_path("keypair_file");
        gen_keypair_file(&keypair_file).unwrap();
        let keypair = read_keypair(&keypair_file).unwrap();
        let test_deactivate_stake =
            test_commands
                .clone()
                .get_matches_from(vec!["test", "deactivate-stake", &keypair_file]);
        assert_eq!(
            parse_command(&pubkey, &test_deactivate_stake).unwrap(),
            WalletCommand::DeactivateStake(keypair)
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
        let node_pubkey = Pubkey::new_rand();
        config.command = WalletCommand::CreateVoteAccount(bob_pubkey, node_pubkey, 0, 10);
        let signature = process_command(&config);
        assert_eq!(signature.unwrap(), SIGNATURE.to_string());

        let bob_keypair = Keypair::new();
        config.command = WalletCommand::AuthorizeVoter(bob_pubkey, bob_keypair, bob_pubkey);
        let signature = process_command(&config);
        assert_eq!(signature.unwrap(), SIGNATURE.to_string());

        config.command = WalletCommand::CreateStakeAccount(bob_pubkey, 10);
        let signature = process_command(&config);
        assert_eq!(signature.unwrap(), SIGNATURE.to_string());

        let bob_keypair = Keypair::new();
        let node_pubkey = Pubkey::new_rand();
        config.command = WalletCommand::DelegateStake(bob_keypair.into(), node_pubkey, 100);
        let signature = process_command(&config);
        assert_eq!(signature.unwrap(), SIGNATURE.to_string());

        let bob_keypair = Keypair::new();
        let to_pubkey = Pubkey::new_rand();
        config.command = WalletCommand::WithdrawStake(bob_keypair.into(), to_pubkey, 100);
        let signature = process_command(&config);
        assert_eq!(signature.unwrap(), SIGNATURE.to_string());

        let bob_keypair = Keypair::new();
        config.command = WalletCommand::DeactivateStake(bob_keypair.into());
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

        config.command = WalletCommand::CreateVoteAccount(bob_pubkey, node_pubkey, 0, 10);
        assert!(process_command(&config).is_err());

        config.command = WalletCommand::AuthorizeVoter(bob_pubkey, Keypair::new(), bob_pubkey);
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

        assert!(program_id.parse::<Pubkey>().is_ok());

        // Failure cases
        config.rpc_client = Some(RpcClient::new_mock("airdrop".to_string()));
        assert!(process_command(&config).is_err());

        config.command = WalletCommand::Deploy("bad/file/location.so".to_string());
        assert!(process_command(&config).is_err());
    }
}
