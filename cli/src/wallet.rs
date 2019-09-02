use crate::display::println_name_value;
use chrono::prelude::*;
use clap::{value_t_or_exit, App, AppSettings, Arg, ArgMatches, SubCommand};
use console::{style, Emoji};
use log::*;
use num_traits::FromPrimitive;
use serde_json;
use serde_json::{json, Value};
use solana_budget_api;
use solana_budget_api::budget_instruction;
use solana_budget_api::budget_state::BudgetError;
use solana_client::client_error::ClientError;
use solana_client::rpc_client::RpcClient;
#[cfg(not(test))]
use solana_drone::drone::request_airdrop_transaction;
#[cfg(test)]
use solana_drone::drone_mock::request_airdrop_transaction;
use solana_sdk::account_utils::State;
use solana_sdk::bpf_loader;
use solana_sdk::fee_calculator::FeeCalculator;
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
use solana_vote_api::vote_state::VoteState;
use std::collections::VecDeque;
use std::fs::File;
use std::io::{Read, Write};
use std::net::{IpAddr, SocketAddr};
use std::thread::sleep;
use std::time::{Duration, Instant};
use std::{error, fmt};

const USERDATA_CHUNK_SIZE: usize = 229; // Keep program chunks under PACKET_DATA_SIZE

static CHECK_MARK: Emoji = Emoji("✅ ", "");
static CROSS_MARK: Emoji = Emoji("❌ ", "");

#[derive(Debug, PartialEq)]
#[allow(clippy::large_enum_variant)]
pub enum WalletCommand {
    Address,
    Fees,
    Airdrop {
        drone_host: Option<IpAddr>,
        drone_port: u16,
        lamports: u64,
    },
    Balance(Pubkey),
    Cancel(Pubkey),
    Confirm(Signature),
    AuthorizeVoter(Pubkey, Keypair, Pubkey),
    CreateVoteAccount(Pubkey, Pubkey, u8, u64),
    ShowAccount(Pubkey, Option<String>),
    ShowVoteAccount(Pubkey),
    DelegateStake(Keypair, Pubkey, u64, bool),
    WithdrawStake(Keypair, Pubkey, u64),
    DeactivateStake(Keypair, Pubkey),
    RedeemVoteCredits(Pubkey, Pubkey),
    ShowStakeAccount(Pubkey),
    CreateReplicatorStorageAccount(Pubkey, Pubkey),
    CreateValidatorStorageAccount(Pubkey, Pubkey),
    ClaimStorageReward(Pubkey, Pubkey),
    ShowStorageAccount(Pubkey),
    Deploy(String),
    GetSlot,
    GetTransactionCount,
    GetVersion,
    // Pay(lamports, to, timestamp, timestamp_pubkey, witness(es), cancelable)
    Pay(
        u64,
        Pubkey,
        Option<DateTime<Utc>>,
        Option<Pubkey>,
        Option<Vec<Pubkey>>,
        Option<Pubkey>,
    ),
    Ping {
        interval: Duration,
        count: Option<u64>,
        timeout: Duration,
    },
    // TimeElapsed(to, process_id, timestamp)
    TimeElapsed(Pubkey, Pubkey, DateTime<Utc>),
    // Witness(to, process_id)
    Witness(Pubkey, Pubkey),
}

#[derive(Debug, Clone)]
pub enum WalletError {
    BadParameter(String),
    CommandNotRecognized(String),
    InsufficientFundsForFee,
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
    pub json_rpc_url: String,
    pub keypair: Keypair,
    pub keypair_path: String,
    pub rpc_client: Option<RpcClient>,
}

impl Default for WalletConfig {
    fn default() -> WalletConfig {
        WalletConfig {
            command: WalletCommand::Balance(Pubkey::default()),
            json_rpc_url: "http://127.0.0.1:8899".to_string(),
            keypair: Keypair::new(),
            keypair_path: "".to_string(),
            rpc_client: None,
        }
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
    if let Some(value) = matches.value_of(name) {
        value.parse::<T>().ok()
    } else {
        None
    }
}

// Return the keypair for an argument with filename `name` or None if not present.
fn keypair_of(matches: &ArgMatches<'_>, name: &str) -> Option<Keypair> {
    if let Some(value) = matches.value_of(name) {
        read_keypair(value).ok()
    } else {
        None
    }
}

// Return a pubkey for an argument that can itself be parsed into a pubkey,
// or is a filename that can be read as a keypair
fn pubkey_of(matches: &ArgMatches<'_>, name: &str) -> Option<Pubkey> {
    value_of(matches, name).or_else(|| keypair_of(matches, name).map(|keypair| keypair.pubkey()))
}

pub fn parse_command(
    pubkey: &Pubkey,
    matches: &ArgMatches<'_>,
) -> Result<WalletCommand, Box<dyn error::Error>> {
    let response = match matches.subcommand() {
        ("address", Some(_address_matches)) => Ok(WalletCommand::Address),
        ("fees", Some(_fees_matches)) => Ok(WalletCommand::Fees),
        ("airdrop", Some(airdrop_matches)) => {
            let drone_port = airdrop_matches
                .value_of("drone_port")
                .unwrap()
                .parse()
                .or_else(|err| {
                    Err(WalletError::BadParameter(format!(
                        "Invalid drone port: {:?}",
                        err
                    )))
                })?;

            let drone_host = if let Some(drone_host) = matches.value_of("drone_host") {
                Some(solana_netutil::parse_host(drone_host).or_else(|err| {
                    Err(WalletError::BadParameter(format!(
                        "Invalid drone host: {:?}",
                        err
                    )))
                })?)
            } else {
                None
            };
            let lamports = airdrop_matches.value_of("lamports").unwrap().parse()?;
            Ok(WalletCommand::Airdrop {
                drone_host,
                drone_port,
                lamports,
            })
        }
        ("balance", Some(balance_matches)) => {
            let pubkey = pubkey_of(&balance_matches, "pubkey").unwrap_or(*pubkey);
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
            let vote_account_pubkey = pubkey_of(matches, "vote_account_pubkey").unwrap();
            let node_pubkey = pubkey_of(matches, "node_pubkey").unwrap();
            let commission = if let Some(commission) = matches.value_of("commission") {
                commission.parse()?
            } else {
                0
            };
            let lamports = matches.value_of("lamports").unwrap().parse()?;
            Ok(WalletCommand::CreateVoteAccount(
                vote_account_pubkey,
                node_pubkey,
                commission,
                lamports,
            ))
        }
        ("authorize-voter", Some(matches)) => {
            let vote_account_pubkey = pubkey_of(matches, "vote_account_pubkey").unwrap();
            let authorized_voter_keypair =
                keypair_of(matches, "authorized_voter_keypair_file").unwrap();
            let new_authorized_voter_pubkey =
                pubkey_of(matches, "new_authorized_voter_pubkey").unwrap();

            Ok(WalletCommand::AuthorizeVoter(
                vote_account_pubkey,
                authorized_voter_keypair,
                new_authorized_voter_pubkey,
            ))
        }
        ("show-account", Some(matches)) => {
            let account_pubkey = pubkey_of(matches, "account_pubkey").unwrap();
            let output_file = matches.value_of("output_file");
            Ok(WalletCommand::ShowAccount(
                account_pubkey,
                output_file.map(ToString::to_string),
            ))
        }
        ("show-vote-account", Some(matches)) => {
            let vote_account_pubkey = pubkey_of(matches, "vote_account_pubkey").unwrap();
            Ok(WalletCommand::ShowVoteAccount(vote_account_pubkey))
        }
        ("delegate-stake", Some(matches)) => {
            let stake_account_keypair = keypair_of(matches, "stake_account_keypair_file").unwrap();
            let vote_account_pubkey = pubkey_of(matches, "vote_account_pubkey").unwrap();
            let lamports_to_stake = matches.value_of("lamports_to_stake").unwrap().parse()?;
            let force = matches.is_present("force");
            Ok(WalletCommand::DelegateStake(
                stake_account_keypair,
                vote_account_pubkey,
                lamports_to_stake,
                force,
            ))
        }
        ("withdraw-stake", Some(matches)) => {
            let stake_account_keypair = keypair_of(matches, "stake_account_keypair_file").unwrap();
            let destination_account_pubkey =
                pubkey_of(matches, "destination_account_pubkey").unwrap();
            let lamports = matches.value_of("lamports").unwrap().parse()?;
            Ok(WalletCommand::WithdrawStake(
                stake_account_keypair,
                destination_account_pubkey,
                lamports,
            ))
        }
        ("deactivate-stake", Some(matches)) => {
            let stake_account_keypair = keypair_of(matches, "stake_account_keypair_file").unwrap();
            let vote_account_pubkey = pubkey_of(matches, "vote_account_pubkey").unwrap();
            Ok(WalletCommand::DeactivateStake(
                stake_account_keypair,
                vote_account_pubkey,
            ))
        }
        ("redeem-vote-credits", Some(matches)) => {
            let stake_account_pubkey = pubkey_of(matches, "stake_account_pubkey").unwrap();
            let vote_account_pubkey = pubkey_of(matches, "vote_account_pubkey").unwrap();
            Ok(WalletCommand::RedeemVoteCredits(
                stake_account_pubkey,
                vote_account_pubkey,
            ))
        }
        ("show-stake-account", Some(matches)) => {
            let stake_account_pubkey = pubkey_of(matches, "stake_account_pubkey").unwrap();
            Ok(WalletCommand::ShowStakeAccount(stake_account_pubkey))
        }
        ("create-replicator-storage-account", Some(matches)) => {
            let account_owner = pubkey_of(matches, "storage_account_owner").unwrap();
            let storage_account_pubkey = pubkey_of(matches, "storage_account_pubkey").unwrap();
            Ok(WalletCommand::CreateReplicatorStorageAccount(
                account_owner,
                storage_account_pubkey,
            ))
        }
        ("create-validator-storage-account", Some(matches)) => {
            let account_owner = pubkey_of(matches, "storage_account_owner").unwrap();
            let storage_account_pubkey = pubkey_of(matches, "storage_account_pubkey").unwrap();
            Ok(WalletCommand::CreateValidatorStorageAccount(
                account_owner,
                storage_account_pubkey,
            ))
        }
        ("claim-storage-reward", Some(matches)) => {
            let node_account_pubkey = pubkey_of(matches, "node_account_pubkey").unwrap();
            let storage_account_pubkey = pubkey_of(matches, "storage_account_pubkey").unwrap();
            Ok(WalletCommand::ClaimStorageReward(
                node_account_pubkey,
                storage_account_pubkey,
            ))
        }
        ("show-storage-account", Some(matches)) => {
            let storage_account_pubkey = pubkey_of(matches, "storage_account_pubkey").unwrap();
            Ok(WalletCommand::ShowStorageAccount(storage_account_pubkey))
        }
        ("deploy", Some(deploy_matches)) => Ok(WalletCommand::Deploy(
            deploy_matches
                .value_of("program_location")
                .unwrap()
                .to_string(),
        )),
        ("get-slot", Some(_matches)) => Ok(WalletCommand::GetSlot),
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
        ("ping", Some(ping_matches)) => {
            let interval = Duration::from_secs(value_t_or_exit!(ping_matches, "interval", u64));
            let count = if ping_matches.is_present("count") {
                Some(value_t_or_exit!(ping_matches, "count", u64))
            } else {
                None
            };
            let timeout = Duration::from_secs(value_t_or_exit!(ping_matches, "timeout", u64));
            Ok(WalletCommand::Ping {
                interval,
                count,
                timeout,
            })
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
        ("cluster-version", Some(_matches)) => Ok(WalletCommand::GetVersion),
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

fn check_account_for_fee(
    rpc_client: &RpcClient,
    config: &WalletConfig,
    fee_calculator: &FeeCalculator,
    message: &Message,
) -> Result<(), Box<dyn error::Error>> {
    check_account_for_multiple_fees(rpc_client, config, fee_calculator, &[message])
}

fn check_account_for_multiple_fees(
    rpc_client: &RpcClient,
    config: &WalletConfig,
    fee_calculator: &FeeCalculator,
    messages: &[&Message],
) -> Result<(), Box<dyn error::Error>> {
    let balance = rpc_client.retry_get_balance(&config.keypair.pubkey(), 5)?;
    if let Some(lamports) = balance {
        if lamports
            >= messages
                .iter()
                .map(|message| fee_calculator.calculate_fee(message))
                .sum()
        {
            return Ok(());
        }
    }
    Err(WalletError::InsufficientFundsForFee)?
}

fn check_unique_pubkeys(
    pubkey0: (&Pubkey, String),
    pubkey1: (&Pubkey, String),
) -> Result<(), WalletError> {
    if pubkey0.0 == pubkey1.0 {
        Err(WalletError::BadParameter(format!(
            "Identical pubkeys found: `{}` and `{}` must be unique",
            pubkey0.1, pubkey1.1
        )))
    } else {
        Ok(())
    }
}

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
    drone_addr: &SocketAddr,
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

    request_and_confirm_airdrop(&rpc_client, drone_addr, &config.keypair.pubkey(), lamports)?;

    let current_balance = rpc_client
        .retry_get_balance(&config.keypair.pubkey(), 5)?
        .unwrap_or(previous_balance);

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
    vote_account_pubkey: &Pubkey,
    node_pubkey: &Pubkey,
    commission: u8,
    lamports: u64,
) -> ProcessResult {
    check_unique_pubkeys(
        (vote_account_pubkey, "vote_account_pubkey".to_string()),
        (node_pubkey, "node_pubkey".to_string()),
    )?;
    check_unique_pubkeys(
        (&config.keypair.pubkey(), "wallet keypair".to_string()),
        (vote_account_pubkey, "vote_account_pubkey".to_string()),
    )?;
    let ixs = vote_instruction::create_account(
        &config.keypair.pubkey(),
        vote_account_pubkey,
        node_pubkey,
        commission,
        lamports,
    );
    let (recent_blockhash, fee_calculator) = rpc_client.get_recent_blockhash()?;
    let mut tx = Transaction::new_signed_instructions(&[&config.keypair], ixs, recent_blockhash);
    check_account_for_fee(rpc_client, config, &fee_calculator, &tx.message)?;
    let result = rpc_client.send_and_confirm_transaction(&mut tx, &[&config.keypair]);
    let signature_str = log_instruction_custom_error::<SystemError>(result)?;
    Ok(signature_str.to_string())
}

fn process_authorize_voter(
    rpc_client: &RpcClient,
    config: &WalletConfig,
    vote_account_pubkey: &Pubkey,
    authorized_voter_keypair: &Keypair,
    new_authorized_voter_pubkey: &Pubkey,
) -> ProcessResult {
    check_unique_pubkeys(
        (vote_account_pubkey, "vote_account_pubkey".to_string()),
        (
            new_authorized_voter_pubkey,
            "new_authorized_voter_pubkey".to_string(),
        ),
    )?;
    let (recent_blockhash, fee_calculator) = rpc_client.get_recent_blockhash()?;
    let ixs = vec![vote_instruction::authorize_voter(
        vote_account_pubkey,                // vote account to update
        &authorized_voter_keypair.pubkey(), // current authorized voter (often the vote account itself)
        new_authorized_voter_pubkey,        // new vote signer
    )];

    let mut tx = Transaction::new_signed_with_payer(
        ixs,
        Some(&config.keypair.pubkey()),
        &[&config.keypair, &authorized_voter_keypair],
        recent_blockhash,
    );
    check_account_for_fee(rpc_client, config, &fee_calculator, &tx.message)?;
    let signature_str = rpc_client
        .send_and_confirm_transaction(&mut tx, &[&config.keypair, &authorized_voter_keypair])?;
    Ok(signature_str.to_string())
}

fn process_show_account(
    rpc_client: &RpcClient,
    _config: &WalletConfig,
    account_pubkey: &Pubkey,
    output_file: &Option<String>,
) -> ProcessResult {
    let account = rpc_client.get_account(account_pubkey)?;

    println!();
    println!("Public Key: {}", account_pubkey);
    println!("Lamports: {}", account.lamports);
    println!("Owner: {}", account.owner);
    println!("Executable: {}", account.executable);

    if let Some(output_file) = output_file {
        let mut f = File::create(output_file)?;
        f.write_all(&account.data)?;
        println!();
        println!("Wrote account data to {}", output_file);
    } else {
        use pretty_hex::*;
        println!("{:?}", account.data.hex_dump());
    }

    Ok("".to_string())
}

fn process_show_vote_account(
    rpc_client: &RpcClient,
    _config: &WalletConfig,
    vote_account_pubkey: &Pubkey,
) -> ProcessResult {
    let vote_account = rpc_client.get_account(vote_account_pubkey)?;

    if vote_account.owner != solana_vote_api::id() {
        Err(WalletError::RpcRequestError(
            format!("{:?} is not a vote account", vote_account_pubkey).to_string(),
        ))?;
    }

    let vote_state = VoteState::deserialize(&vote_account.data).map_err(|_| {
        WalletError::RpcRequestError(
            "Account data could not be deserialized to vote state".to_string(),
        )
    })?;

    println!("account lamports: {}", vote_account.lamports);
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

fn process_deactivate_stake_account(
    rpc_client: &RpcClient,
    config: &WalletConfig,
    stake_account_keypair: &Keypair,
    vote_account_pubkey: &Pubkey,
) -> ProcessResult {
    let (recent_blockhash, fee_calculator) = rpc_client.get_recent_blockhash()?;
    let ixs =
        stake_instruction::deactivate_stake(&stake_account_keypair.pubkey(), vote_account_pubkey);
    let mut tx = Transaction::new_signed_with_payer(
        vec![ixs],
        Some(&config.keypair.pubkey()),
        &[&config.keypair, &stake_account_keypair],
        recent_blockhash,
    );
    check_account_for_fee(rpc_client, config, &fee_calculator, &tx.message)?;
    let signature_str = rpc_client
        .send_and_confirm_transaction(&mut tx, &[&config.keypair, &stake_account_keypair])?;
    Ok(signature_str.to_string())
}

fn process_delegate_stake(
    rpc_client: &RpcClient,
    config: &WalletConfig,
    stake_account_keypair: &Keypair,
    vote_account_pubkey: &Pubkey,
    lamports: u64,
    force: bool,
) -> ProcessResult {
    check_unique_pubkeys(
        (&config.keypair.pubkey(), "wallet keypair".to_string()),
        (
            &stake_account_keypair.pubkey(),
            "stake_account_keypair".to_string(),
        ),
    )?;
    let (recent_blockhash, fee_calculator) = rpc_client.get_recent_blockhash()?;

    let ixs = stake_instruction::create_stake_account_and_delegate_stake(
        &config.keypair.pubkey(),
        &stake_account_keypair.pubkey(),
        vote_account_pubkey,
        lamports,
    );

    // Sanity check the vote account to ensure it is attached to a validator that has recently
    // voted at the tip of the ledger
    let vote_account_data = rpc_client
        .get_account_data(vote_account_pubkey)
        .map_err(|_| {
            WalletError::RpcRequestError(format!("Vote account not found: {}", vote_account_pubkey))
        })?;

    let vote_state = VoteState::deserialize(&vote_account_data).map_err(|_| {
        WalletError::RpcRequestError(
            "Account data could not be deserialized to vote state".to_string(),
        )
    })?;

    let sanity_check_result = match vote_state.root_slot {
        None => Err(WalletError::BadParameter(
            "Unable to delegate. Vote account has no root slot".to_string(),
        )),
        Some(root_slot) => {
            let slot = rpc_client.get_slot()?;
            if root_slot + solana_sdk::timing::DEFAULT_SLOTS_PER_TURN < slot {
                Err(WalletError::BadParameter(
                    format!(
                    "Unable to delegate. Vote account root slot ({}) is too old, the current slot is {}", root_slot, slot
                    )
                ))
            } else {
                Ok(())
            }
        }
    };

    if sanity_check_result.is_err() {
        if !force {
            sanity_check_result?;
        } else {
            println!("--force supplied, ignoring: {:?}", sanity_check_result);
        }
    }

    let mut tx = Transaction::new_signed_with_payer(
        ixs,
        Some(&config.keypair.pubkey()),
        &[&config.keypair, &stake_account_keypair],
        recent_blockhash,
    );
    check_account_for_fee(rpc_client, config, &fee_calculator, &tx.message)?;

    let result = rpc_client
        .send_and_confirm_transaction(&mut tx, &[&config.keypair, &stake_account_keypair]);
    let signature_str = log_instruction_custom_error::<SystemError>(result)?;
    Ok(signature_str.to_string())
}

fn process_withdraw_stake(
    rpc_client: &RpcClient,
    config: &WalletConfig,
    stake_account_keypair: &Keypair,
    destination_account_pubkey: &Pubkey,
    lamports: u64,
) -> ProcessResult {
    let (recent_blockhash, fee_calculator) = rpc_client.get_recent_blockhash()?;
    let ixs = vec![stake_instruction::withdraw(
        &stake_account_keypair.pubkey(),
        destination_account_pubkey,
        lamports,
    )];

    let mut tx = Transaction::new_signed_with_payer(
        ixs,
        Some(&config.keypair.pubkey()),
        &[&config.keypair, &stake_account_keypair],
        recent_blockhash,
    );
    check_account_for_fee(rpc_client, config, &fee_calculator, &tx.message)?;

    let signature_str = rpc_client
        .send_and_confirm_transaction(&mut tx, &[&config.keypair, &stake_account_keypair])?;
    Ok(signature_str.to_string())
}

fn process_redeem_vote_credits(
    rpc_client: &RpcClient,
    config: &WalletConfig,
    stake_account_pubkey: &Pubkey,
    vote_account_pubkey: &Pubkey,
) -> ProcessResult {
    let (recent_blockhash, fee_calculator) = rpc_client.get_recent_blockhash()?;
    let ixs = vec![stake_instruction::redeem_vote_credits(
        stake_account_pubkey,
        vote_account_pubkey,
    )];
    let mut tx = Transaction::new_signed_with_payer(
        ixs,
        Some(&config.keypair.pubkey()),
        &[&config.keypair],
        recent_blockhash,
    );
    check_account_for_fee(rpc_client, config, &fee_calculator, &tx.message)?;
    let signature_str = rpc_client.send_and_confirm_transaction(&mut tx, &[&config.keypair])?;
    Ok(signature_str.to_string())
}

fn process_show_stake_account(
    rpc_client: &RpcClient,
    _config: &WalletConfig,
    stake_account_pubkey: &Pubkey,
) -> ProcessResult {
    use solana_stake_api::stake_state::StakeState;
    let stake_account = rpc_client.get_account(stake_account_pubkey)?;
    if stake_account.owner != solana_stake_api::id() {
        Err(WalletError::RpcRequestError(
            format!("{:?} is not a stake account", stake_account_pubkey).to_string(),
        ))?;
    }
    match stake_account.state() {
        Ok(StakeState::Stake(stake)) => {
            println!("total stake: {}", stake_account.lamports);
            println!("credits observed: {}", stake.credits_observed);
            println!("delegated stake: {}", stake.stake);
            if stake.voter_pubkey != Pubkey::default() {
                println!("delegated voter pubkey: {}", stake.voter_pubkey);
            }
            println!(
                "stake activates starting from epoch: {}",
                stake.activation_epoch
            );
            if stake.deactivation_epoch < std::u64::MAX {
                println!(
                    "stake deactivates starting from epoch: {}",
                    stake.deactivation_epoch
                );
            }
            Ok("".to_string())
        }
        Ok(StakeState::RewardsPool) => Ok("Stake account is a rewards pool".to_string()),
        Ok(StakeState::Uninitialized) => Ok("Stake account is uninitialized".to_string()),
        Err(err) => Err(WalletError::RpcRequestError(format!(
            "Account data could not be deserialized to stake state: {:?}",
            err
        )))?,
    }
}

fn process_create_replicator_storage_account(
    rpc_client: &RpcClient,
    config: &WalletConfig,
    account_owner: &Pubkey,
    storage_account_pubkey: &Pubkey,
) -> ProcessResult {
    check_unique_pubkeys(
        (&config.keypair.pubkey(), "wallet keypair".to_string()),
        (
            &storage_account_pubkey,
            "storage_account_pubkey".to_string(),
        ),
    )?;
    let (recent_blockhash, fee_calculator) = rpc_client.get_recent_blockhash()?;
    let ixs = storage_instruction::create_replicator_storage_account(
        &config.keypair.pubkey(),
        &account_owner,
        storage_account_pubkey,
        1,
    );
    let mut tx = Transaction::new_signed_instructions(&[&config.keypair], ixs, recent_blockhash);
    check_account_for_fee(rpc_client, config, &fee_calculator, &tx.message)?;
    let result = rpc_client.send_and_confirm_transaction(&mut tx, &[&config.keypair]);
    let signature_str = log_instruction_custom_error::<SystemError>(result)?;
    Ok(signature_str.to_string())
}

fn process_create_validator_storage_account(
    rpc_client: &RpcClient,
    config: &WalletConfig,
    account_owner: &Pubkey,
    storage_account_pubkey: &Pubkey,
) -> ProcessResult {
    check_unique_pubkeys(
        (&config.keypair.pubkey(), "wallet keypair".to_string()),
        (
            &storage_account_pubkey,
            "storage_account_pubkey".to_string(),
        ),
    )?;
    let (recent_blockhash, fee_calculator) = rpc_client.get_recent_blockhash()?;
    let ixs = storage_instruction::create_validator_storage_account(
        &config.keypair.pubkey(),
        account_owner,
        storage_account_pubkey,
        1,
    );
    let mut tx = Transaction::new_signed_instructions(&[&config.keypair], ixs, recent_blockhash);
    check_account_for_fee(rpc_client, config, &fee_calculator, &tx.message)?;
    let result = rpc_client.send_and_confirm_transaction(&mut tx, &[&config.keypair]);
    let signature_str = log_instruction_custom_error::<SystemError>(result)?;
    Ok(signature_str.to_string())
}

fn process_claim_storage_reward(
    rpc_client: &RpcClient,
    config: &WalletConfig,
    node_account_pubkey: &Pubkey,
    storage_account_pubkey: &Pubkey,
) -> ProcessResult {
    let (recent_blockhash, fee_calculator) = rpc_client.get_recent_blockhash()?;

    let instruction =
        storage_instruction::claim_reward(node_account_pubkey, storage_account_pubkey);
    let signers = [&config.keypair];
    let message = Message::new_with_payer(vec![instruction], Some(&signers[0].pubkey()));

    let mut tx = Transaction::new(&signers, message, recent_blockhash);
    check_account_for_fee(rpc_client, config, &fee_calculator, &tx.message)?;
    let signature_str = rpc_client.send_and_confirm_transaction(&mut tx, &signers)?;
    Ok(signature_str.to_string())
}

fn process_show_storage_account(
    rpc_client: &RpcClient,
    _config: &WalletConfig,
    storage_account_pubkey: &Pubkey,
) -> ProcessResult {
    let account = rpc_client.get_account(storage_account_pubkey)?;

    if account.owner != solana_storage_api::id() {
        Err(WalletError::RpcRequestError(
            format!("{:?} is not a storage account", storage_account_pubkey).to_string(),
        ))?;
    }

    use solana_storage_api::storage_contract::StorageContract;
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

    // Build transactions to calculate fees
    let mut messages: Vec<&Message> = Vec::new();
    let (blockhash, fee_calculator) = rpc_client.get_recent_blockhash()?;
    let mut create_account_tx = system_transaction::create_account(
        &config.keypair,
        &program_id.pubkey(),
        blockhash,
        1,
        program_data.len() as u64,
        &bpf_loader::id(),
    );
    messages.push(&create_account_tx.message);
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
    for transaction in write_transactions.iter() {
        messages.push(&transaction.message);
    }

    let instruction = loader_instruction::finalize(&program_id.pubkey(), &bpf_loader::id());
    let message = Message::new_with_payer(vec![instruction], Some(&signers[0].pubkey()));
    let mut finalize_tx = Transaction::new(&signers, message, blockhash);
    messages.push(&finalize_tx.message);

    check_account_for_multiple_fees(rpc_client, config, &fee_calculator, &messages)?;

    trace!("Creating program account");
    let result =
        rpc_client.send_and_confirm_transaction(&mut create_account_tx, &[&config.keypair]);
    log_instruction_custom_error::<SystemError>(result).map_err(|_| {
        WalletError::DynamicProgramError("Program allocate space failed".to_string())
    })?;

    trace!("Writing program data");
    rpc_client.send_and_confirm_transactions(write_transactions, &signers)?;

    trace!("Finalizing program account");
    rpc_client
        .send_and_confirm_transaction(&mut finalize_tx, &signers)
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
    check_unique_pubkeys(
        (&config.keypair.pubkey(), "wallet keypair".to_string()),
        (to, "to".to_string()),
    )?;
    let (blockhash, fee_calculator) = rpc_client.get_recent_blockhash()?;

    if timestamp == None && *witnesses == None {
        let mut tx = system_transaction::transfer(&config.keypair, to, lamports, blockhash);
        check_account_for_fee(rpc_client, config, &fee_calculator, &tx.message)?;
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
        check_account_for_fee(rpc_client, config, &fee_calculator, &tx.message)?;
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
        check_account_for_fee(rpc_client, config, &fee_calculator, &tx.message)?;
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
    let (blockhash, fee_calculator) = rpc_client.get_recent_blockhash()?;
    let ix = budget_instruction::apply_signature(
        &config.keypair.pubkey(),
        pubkey,
        &config.keypair.pubkey(),
    );
    let mut tx = Transaction::new_signed_instructions(&[&config.keypair], vec![ix], blockhash);
    check_account_for_fee(rpc_client, config, &fee_calculator, &tx.message)?;
    let result = rpc_client.send_and_confirm_transaction(&mut tx, &[&config.keypair]);
    let signature_str = log_instruction_custom_error::<BudgetError>(result)?;
    Ok(signature_str.to_string())
}

fn process_get_slot(rpc_client: &RpcClient) -> ProcessResult {
    let slot = rpc_client.get_slot()?;
    Ok(slot.to_string())
}

fn process_get_transaction_count(rpc_client: &RpcClient) -> ProcessResult {
    let transaction_count = rpc_client.get_transaction_count()?;
    Ok(transaction_count.to_string())
}

fn process_time_elapsed(
    rpc_client: &RpcClient,
    config: &WalletConfig,
    to: &Pubkey,
    pubkey: &Pubkey,
    dt: DateTime<Utc>,
) -> ProcessResult {
    let (blockhash, fee_calculator) = rpc_client.get_recent_blockhash()?;

    let ix = budget_instruction::apply_timestamp(&config.keypair.pubkey(), pubkey, to, dt);
    let mut tx = Transaction::new_signed_instructions(&[&config.keypair], vec![ix], blockhash);
    check_account_for_fee(rpc_client, config, &fee_calculator, &tx.message)?;
    let result = rpc_client.send_and_confirm_transaction(&mut tx, &[&config.keypair]);
    let signature_str = log_instruction_custom_error::<BudgetError>(result)?;

    Ok(signature_str.to_string())
}

fn process_witness(
    rpc_client: &RpcClient,
    config: &WalletConfig,
    to: &Pubkey,
    pubkey: &Pubkey,
) -> ProcessResult {
    let (blockhash, fee_calculator) = rpc_client.get_recent_blockhash()?;

    let ix = budget_instruction::apply_signature(&config.keypair.pubkey(), pubkey, to);
    let mut tx = Transaction::new_signed_instructions(&[&config.keypair], vec![ix], blockhash);
    check_account_for_fee(rpc_client, config, &fee_calculator, &tx.message)?;
    let result = rpc_client.send_and_confirm_transaction(&mut tx, &[&config.keypair]);
    let signature_str = log_instruction_custom_error::<BudgetError>(result)?;

    Ok(signature_str.to_string())
}

fn process_get_version(rpc_client: &RpcClient, config: &WalletConfig) -> ProcessResult {
    let remote_version: Value = serde_json::from_str(&rpc_client.get_version()?)?;
    println!(
        "{} {}",
        style("Cluster versions from:").bold(),
        config.json_rpc_url
    );
    if let Some(versions) = remote_version.as_object() {
        for (key, value) in versions.iter() {
            if let Some(value_string) = value.as_str() {
                println_name_value(&format!("* {}:", key), &value_string);
            }
        }
    }
    Ok("".to_string())
}

fn process_ping(
    rpc_client: &RpcClient,
    config: &WalletConfig,
    interval: &Duration,
    count: &Option<u64>,
    timeout: &Duration,
) -> ProcessResult {
    let to = Keypair::new().pubkey();

    println_name_value("Source account:", &config.keypair.pubkey().to_string());
    println_name_value("Destination account:", &to.to_string());
    println!();

    let (signal_sender, signal_receiver) = std::sync::mpsc::channel();
    ctrlc::set_handler(move || {
        let _ = signal_sender.send(());
    })
    .expect("Error setting Ctrl-C handler");

    let mut last_blockhash = Hash::default();
    let mut submit_count = 0;
    let mut confirmed_count = 0;
    let mut confirmation_time: VecDeque<u64> = VecDeque::with_capacity(1024);

    'mainloop: for seq in 0..count.unwrap_or(std::u64::MAX) {
        let (recent_blockhash, fee_calculator) = rpc_client.get_new_blockhash(&last_blockhash)?;
        last_blockhash = recent_blockhash;

        let transaction = system_transaction::transfer(&config.keypair, &to, 1, recent_blockhash);
        check_account_for_fee(rpc_client, config, &fee_calculator, &transaction.message)?;

        match rpc_client.send_transaction(&transaction) {
            Ok(signature) => {
                let transaction_sent = Instant::now();
                loop {
                    let signature_status = rpc_client.get_signature_status(&signature)?;
                    let elapsed_time = Instant::now().duration_since(transaction_sent);
                    if let Some(transaction_status) = signature_status {
                        match transaction_status {
                            Ok(()) => {
                                let elapsed_time_millis = elapsed_time.as_millis() as u64;
                                confirmation_time.push_back(elapsed_time_millis);
                                println!(
                                    "{}1 lamport transferred: seq={:<3} time={:>4}ms signature={}",
                                    CHECK_MARK, seq, elapsed_time_millis, signature
                                );
                                confirmed_count += 1;
                            }
                            Err(err) => {
                                println!(
                                    "{}Transaction failed:    seq={:<3} error={:?} signature={}",
                                    CROSS_MARK, seq, err, signature
                                );
                            }
                        }
                        break;
                    }

                    if elapsed_time >= *timeout {
                        println!(
                            "{}Confirmation timeout:  seq={:<3}             signature={}",
                            CROSS_MARK, seq, signature
                        );
                        break;
                    }

                    // Sleep for half a slot
                    if signal_receiver
                        .recv_timeout(Duration::from_millis(
                            500 * solana_sdk::timing::DEFAULT_TICKS_PER_SLOT
                                / solana_sdk::timing::DEFAULT_TICKS_PER_SECOND,
                        ))
                        .is_ok()
                    {
                        break 'mainloop;
                    }
                }
            }
            Err(err) => {
                println!(
                    "{}Submit failed:         seq={:<3} error={:?}",
                    CROSS_MARK, seq, err
                );
            }
        }
        submit_count += 1;

        if signal_receiver.recv_timeout(*interval).is_ok() {
            break 'mainloop;
        }
    }

    println!();
    println!("--- transaction statistics ---");
    println!(
        "{} transactions submitted, {} transactions confirmed, {:.1}% transaction loss",
        submit_count,
        confirmed_count,
        (100. - f64::from(confirmed_count) / f64::from(submit_count) * 100.)
    );
    if !confirmation_time.is_empty() {
        let samples: Vec<f64> = confirmation_time.iter().map(|t| *t as f64).collect();
        let dist = criterion_stats::Distribution::from(samples.into_boxed_slice());
        let mean = dist.mean();
        println!(
            "confirmation min/mean/max/stddev = {:.0}/{:.0}/{:.0}/{:.0} ms",
            dist.min(),
            mean,
            dist.max(),
            dist.std_dev(Some(mean))
        );
    }

    Ok("".to_string())
}

pub fn process_command(config: &WalletConfig) -> ProcessResult {
    println_name_value("Keypair:", &config.keypair_path);
    if let WalletCommand::Address = config.command {
        // Get address of this client
        return Ok(format!("{}", config.keypair.pubkey()));
    }
    println_name_value("RPC Endpoint:", &config.json_rpc_url);

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
        WalletCommand::Airdrop {
            drone_host,
            drone_port,
            lamports,
        } => {
            let drone_addr = SocketAddr::new(
                drone_host.unwrap_or_else(|| {
                    let drone_host = url::Url::parse(&config.json_rpc_url)
                        .unwrap()
                        .host()
                        .unwrap()
                        .to_string();
                    solana_netutil::parse_host(&drone_host).unwrap_or_else(|err| {
                        panic!("Unable to resolve {}: {}", drone_host, err);
                    })
                }),
                *drone_port,
            );

            process_airdrop(&rpc_client, config, &drone_addr, *lamports)
        }

        // Check client balance
        WalletCommand::Balance(pubkey) => process_balance(&pubkey, &rpc_client),

        // Cancel a contract by contract Pubkey
        WalletCommand::Cancel(pubkey) => process_cancel(&rpc_client, config, &pubkey),

        // Confirm the last client transaction by signature
        WalletCommand::Confirm(signature) => process_confirm(&rpc_client, signature),

        // Create vote account
        WalletCommand::CreateVoteAccount(
            vote_account_pubkey,
            node_pubkey,
            commission,
            lamports,
        ) => process_create_vote_account(
            &rpc_client,
            config,
            &vote_account_pubkey,
            &node_pubkey,
            *commission,
            *lamports,
        ),

        WalletCommand::AuthorizeVoter(
            vote_account_pubkey,
            authorized_voter_keypair,
            new_authorized_voter_pubkey,
        ) => process_authorize_voter(
            &rpc_client,
            config,
            &vote_account_pubkey,
            &authorized_voter_keypair,
            &new_authorized_voter_pubkey,
        ),

        WalletCommand::ShowAccount(account_pubkey, output_file) => {
            process_show_account(&rpc_client, config, &account_pubkey, &output_file)
        }

        WalletCommand::ShowVoteAccount(vote_account_pubkey) => {
            process_show_vote_account(&rpc_client, config, &vote_account_pubkey)
        }

        WalletCommand::DelegateStake(
            stake_account_keypair,
            vote_account_pubkey,
            lamports,
            force,
        ) => process_delegate_stake(
            &rpc_client,
            config,
            &stake_account_keypair,
            &vote_account_pubkey,
            *lamports,
            *force,
        ),

        WalletCommand::WithdrawStake(
            stake_account_keypair,
            destination_account_pubkey,
            lamports,
        ) => process_withdraw_stake(
            &rpc_client,
            config,
            &stake_account_keypair,
            &destination_account_pubkey,
            *lamports,
        ),

        // Deactivate stake account
        WalletCommand::DeactivateStake(stake_account_keypair, vote_account_pubkey) => {
            process_deactivate_stake_account(
                &rpc_client,
                config,
                &stake_account_keypair,
                &vote_account_pubkey,
            )
        }

        WalletCommand::RedeemVoteCredits(stake_account_pubkey, vote_account_pubkey) => {
            process_redeem_vote_credits(
                &rpc_client,
                config,
                &stake_account_pubkey,
                &vote_account_pubkey,
            )
        }

        WalletCommand::ShowStakeAccount(stake_account_pubkey) => {
            process_show_stake_account(&rpc_client, config, &stake_account_pubkey)
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

        WalletCommand::GetSlot => process_get_slot(&rpc_client),
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

        WalletCommand::Ping {
            interval,
            count,
            timeout,
        } => process_ping(&rpc_client, config, interval, count, timeout),

        // Apply time elapsed to contract
        WalletCommand::TimeElapsed(to, pubkey, dt) => {
            process_time_elapsed(&rpc_client, config, &to, &pubkey, *dt)
        }

        // Apply witness signature to contract
        WalletCommand::Witness(to, pubkey) => process_witness(&rpc_client, config, &to, &pubkey),

        // Return software version of wallet and cluster entrypoint node
        WalletCommand::GetVersion => process_get_version(&rpc_client, config),
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

// Return an error if a keypair file cannot be parsed.
fn is_keypair(string: String) -> Result<(), String> {
    read_keypair(&string)
        .map(|_| ())
        .map_err(|err| format!("{:?}", err))
}

// Return an error if string cannot be parsed as pubkey string or keypair file location
fn is_pubkey_or_keypair(string: String) -> Result<(), String> {
    is_pubkey(string.clone()).or_else(|_| is_keypair(string))
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
                .about("Request lamports")
                .arg(
                    Arg::with_name("drone_host")
                        .long("drone-host")
                        .value_name("HOST")
                        .takes_value(true)
                        .help("Drone host to use [default: the --url host]"),
                )
                .arg(
                    Arg::with_name("drone_port")
                        .long("drone-port")
                        .value_name("PORT")
                        .takes_value(true)
                        .default_value(solana_drone::drone::DRONE_PORT_STR)
                        .help("Drone port to use"),
                )
                .arg(
                    Arg::with_name("lamports")
                        .index(1)
                        .value_name("LAMPORTS")
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
                        .validator(is_pubkey_or_keypair)
                        .help("The public key of the balance to check"),
                ),
        )
        .subcommand(
            SubCommand::with_name("cancel")
                .about("Cancel a transfer")
                .arg(
                    Arg::with_name("process_id")
                        .index(1)
                        .value_name("PROCESS ID")
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
                    Arg::with_name("vote_account_pubkey")
                        .index(1)
                        .value_name("VOTE ACCOUNT PUBKEY")
                        .takes_value(true)
                        .required(true)
                        .validator(is_pubkey_or_keypair)
                        .help("Vote account in which to set the authorized voter"),
                )
                .arg(
                    Arg::with_name("authorized_voter_keypair_file")
                        .index(2)
                        .value_name("CURRENT VOTER KEYPAIR FILE")
                        .takes_value(true)
                        .required(true)
                        .validator(is_keypair)
                        .help("Keypair file for the currently authorized vote signer"),
                )
                .arg(
                    Arg::with_name("new_authorized_voter_pubkey")
                        .index(3)
                        .value_name("NEW VOTER PUBKEY")
                        .takes_value(true)
                        .required(true)
                        .validator(is_pubkey_or_keypair)
                        .help("New vote signer to authorize"),
                ),
        )
        .subcommand(
            SubCommand::with_name("create-vote-account")
                .about("Create a vote account")
                .arg(
                    Arg::with_name("vote_account_pubkey")
                        .index(1)
                        .value_name("VOTE ACCOUNT PUBKEY")
                        .takes_value(true)
                        .required(true)
                        .validator(is_pubkey_or_keypair)
                        .help("Vote account address to fund"),
                )
                .arg(
                    Arg::with_name("node_pubkey")
                        .index(2)
                        .value_name("VALIDATOR PUBKEY")
                        .takes_value(true)
                        .required(true)
                        .validator(is_pubkey_or_keypair)
                        .help("Validator that will vote with this account"),
                )
                .arg(
                    Arg::with_name("lamports")
                        .index(3)
                        .value_name("LAMPORTS")
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
            SubCommand::with_name("show-account")
                .about("Show the contents of an account")
                .arg(
                    Arg::with_name("account_pubkey")
                        .index(1)
                        .value_name("ACCOUNT PUBKEY")
                        .takes_value(true)
                        .required(true)
                        .validator(is_pubkey_or_keypair)
                        .help("Account pubkey"),
                )
                .arg(
                    Arg::with_name("output_file")
                        .long("output")
                        .short("o")
                        .value_name("FILE")
                        .takes_value(true)
                        .help("Write the account data to this file"),
                ),
        )
        .subcommand(
            SubCommand::with_name("show-vote-account")
                .about("Show the contents of a vote account")
                .arg(
                    Arg::with_name("vote_account_pubkey")
                        .index(1)
                        .value_name("VOTE ACCOUNT PUBKEY")
                        .takes_value(true)
                        .required(true)
                        .validator(is_pubkey_or_keypair)
                        .help("Vote account pubkey"),
                ),
        )
        .subcommand(
            SubCommand::with_name("delegate-stake")
                .about("Delegate stake to a vote account")
                .arg(
                    Arg::with_name("force")
                        .long("force")
                        .takes_value(false)
                        .hidden(true) // Don't document this argument to discourage its use
                        .help("Override vote account sanity checks (use carefully!)"),
                )
                .arg(
                    Arg::with_name("stake_account_keypair_file")
                        .index(1)
                        .value_name("STAKE ACCOUNT KEYPAIR FILE")
                        .takes_value(true)
                        .required(true)
                        .validator(is_keypair)
                        .help("Keypair file for the new stake account"),
                )
                .arg(
                    Arg::with_name("vote_account_pubkey")
                        .index(2)
                        .value_name("VOTE ACCOUNT PUBKEY")
                        .takes_value(true)
                        .required(true)
                        .validator(is_pubkey_or_keypair)
                        .help("The vote account to which the stake will be delegated"),
                )
                .arg(
                    Arg::with_name("lamports_to_stake")
                        .index(3)
                        .value_name("LAMPORTS")
                        .takes_value(true)
                        .required(true)
                        .help("The number of lamports to stake"),
                ),
        )
        .subcommand(
            SubCommand::with_name("deactivate-stake")
                .about("Deactivate the delegated stake from the stake account")
                .arg(
                    Arg::with_name("stake_account_keypair_file")
                        .index(1)
                        .value_name("STAKE ACCOUNT KEYPAIR FILE")
                        .takes_value(true)
                        .required(true)
                        .help("Keypair file for the stake account, for signing the delegate transaction."),
                )
                .arg(
                    Arg::with_name("vote_account_pubkey")
                        .index(2)
                        .value_name("PUBKEY")
                        .takes_value(true)
                        .required(true)
                        .validator(is_pubkey_or_keypair)
                        .help("The vote account to which the stake is currently delegated"),
                )
        )
        .subcommand(
            SubCommand::with_name("withdraw-stake")
                .about("Withdraw the unstaked lamports from the stake account")
                .arg(
                    Arg::with_name("stake_account_keypair_file")
                        .index(1)
                        .value_name("STAKE ACCOUNT KEYPAIR FILE")
                        .takes_value(true)
                        .required(true)
                        .validator(is_keypair)
                        .help("Keypair file for the stake account, for signing the withdraw transaction."),
                )
                .arg(
                    Arg::with_name("destination_account_pubkey")
                        .index(2)
                        .value_name("DESTINATION PUBKEY")
                        .takes_value(true)
                        .required(true)
                        .validator(is_pubkey_or_keypair)
                        .help("The account where the lamports should be transfered"),
                )
                .arg(
                    Arg::with_name("lamports")
                        .index(3)
                        .value_name("LAMPORTS")
                        .takes_value(true)
                        .required(true)
                        .help("The number of lamports to withdraw from the stake account."),
                ),
        )
        .subcommand(
            SubCommand::with_name("redeem-vote-credits")
                .about("Redeem credits in the stake account")
                .arg(
                    Arg::with_name("stake_account_pubkey")
                        .index(1)
                        .value_name("STAKING ACCOUNT PUBKEY")
                        .takes_value(true)
                        .required(true)
                        .validator(is_pubkey_or_keypair)
                        .help("Staking account address to redeem credits for"),
                )
                .arg(
                    Arg::with_name("vote_account_pubkey")
                        .index(2)
                        .value_name("VOTE ACCOUNT PUBKEY")
                        .takes_value(true)
                        .required(true)
                        .validator(is_pubkey_or_keypair)
                        .help("The vote account to which the stake was previously delegated."),
                ),
        )
        .subcommand(
            SubCommand::with_name("show-stake-account")
                .about("Show the contents of a stake account")
                .arg(
                    Arg::with_name("stake_account_pubkey")
                        .index(1)
                        .value_name("STAKE ACCOUNT PUBKEY")
                        .takes_value(true)
                        .required(true)
                        .validator(is_pubkey_or_keypair)
                        .help("Stake account pubkey"),
                )
        )
        .subcommand(
            SubCommand::with_name("create-storage-mining-pool-account")
                .about("Create mining pool account")
                .arg(
                    Arg::with_name("storage_account_pubkey")
                        .index(1)
                        .value_name("STORAGE ACCOUNT PUBKEY")
                        .takes_value(true)
                        .required(true)
                        .validator(is_pubkey_or_keypair)
                        .help("Storage mining pool account address to fund"),
                )
                .arg(
                    Arg::with_name("lamports")
                        .index(2)
                        .value_name("LAMPORTS")
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
                        .value_name("STORAGE ACCOUNT OWNER PUBKEY")
                        .takes_value(true)
                        .required(true)
                        .validator(is_pubkey_or_keypair)
                )
                .arg(
                    Arg::with_name("storage_account_pubkey")
                        .index(2)
                        .value_name("STORAGE ACCOUNT PUBKEY")
                        .takes_value(true)
                        .required(true)
                        .validator(is_pubkey_or_keypair)
                )
        )
        .subcommand(
            SubCommand::with_name("create-validator-storage-account")
                .about("Create a validator storage account")
                .arg(
                    Arg::with_name("storage_account_owner")
                        .index(1)
                        .value_name("STORAGE ACCOUNT OWNER PUBKEY")
                        .takes_value(true)
                        .required(true)
                        .validator(is_pubkey_or_keypair)
                )
                .arg(
                    Arg::with_name("storage_account_pubkey")
                        .index(2)
                        .value_name("STORAGE ACCOUNT PUBKEY")
                        .takes_value(true)
                        .required(true)
                        .validator(is_pubkey_or_keypair)
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
                        .validator(is_pubkey_or_keypair)
                        .help("The node account to credit the rewards to"),
                )
                .arg(
                    Arg::with_name("storage_account_pubkey")
                        .index(2)
                        .value_name("STORAGE ACCOUNT PUBKEY")
                        .takes_value(true)
                        .required(true)
                        .validator(is_pubkey_or_keypair)
                        .help("Storage account address to redeem credits for"),
                ))
        .subcommand(
            SubCommand::with_name("show-storage-account")
                .about("Show the contents of a storage account")
                .arg(
                    Arg::with_name("storage_account_pubkey")
                        .index(1)
                        .value_name("STORAGE ACCOUNT PUBKEY")
                        .takes_value(true)
                        .required(true)
                        .validator(is_pubkey_or_keypair)
                        .help("Storage account pubkey"),
                )
        )
        .subcommand(
            SubCommand::with_name("deploy")
                .about("Deploy a program")
                .arg(
                    Arg::with_name("program_location")
                        .index(1)
                        .value_name("PATH TO PROGRAM")
                        .takes_value(true)
                        .required(true)
                        .help("/path/to/program.o"),
                ), // TODO: Add "loader" argument; current default is bpf_loader
        )
        .subcommand(
            SubCommand::with_name("get-slot")
                .about("Get current slot"),
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
                        .value_name("LAMPORTS")
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
            SubCommand::with_name("ping")
                .about("Submit transactions sequentially")
                .arg(
                    Arg::with_name("interval")
                        .short("i")
                        .long("interval")
                        .value_name("SECONDS")
                        .takes_value(true)
                        .default_value("2")
                        .help("Wait interval seconds between submitting the next transaction"),
                )
                .arg(
                    Arg::with_name("count")
                        .short("c")
                        .long("count")
                        .value_name("NUMBER")
                        .takes_value(true)
                        .help("Stop after submitting count transactions"),
                )
                .arg(
                    Arg::with_name("timeout")
                        .short("t")
                        .long("timeout")
                        .value_name("SECONDS")
                        .takes_value(true)
                        .default_value("10")
                        .help("Wait up to timeout seconds for transaction confirmation"),
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
                        .value_name("PROCESS ID")
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
                        .value_name("PROCESS ID")
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
        .subcommand(
            SubCommand::with_name("cluster-version")
                .about("Get the version of the cluster entrypoint"),
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use solana_client::mock_rpc_client_request::SIGNATURE;
    use solana_sdk::signature::gen_keypair_file;
    use solana_sdk::transaction::TransactionError;
    use std::path::PathBuf;

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
            WalletCommand::Airdrop {
                drone_host: None,
                drone_port: solana_drone::drone::DRONE_PORT,
                lamports: 50
            }
        );
        let test_bad_airdrop = test_commands
            .clone()
            .get_matches_from(vec!["test", "airdrop", "notint"]);
        assert!(parse_command(&pubkey, &test_bad_airdrop).is_err());

        // Test Balance Subcommand, incl pubkey and keypair-file inputs
        let keypair_file = make_tmp_path("keypair_file");
        gen_keypair_file(&keypair_file).unwrap();
        let keypair = read_keypair(&keypair_file).unwrap();
        let test_balance = test_commands.clone().get_matches_from(vec![
            "test",
            "balance",
            &keypair.pubkey().to_string(),
        ]);
        assert_eq!(
            parse_command(&pubkey, &test_balance).unwrap(),
            WalletCommand::Balance(keypair.pubkey())
        );
        let test_balance =
            test_commands
                .clone()
                .get_matches_from(vec!["test", "balance", &keypair_file]);
        assert_eq!(
            parse_command(&pubkey, &test_balance).unwrap(),
            WalletCommand::Balance(keypair.pubkey())
        );

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

        // Test DelegateStake Subcommand
        fn make_tmp_path(name: &str) -> String {
            let out_dir = std::env::var("FARF_DIR").unwrap_or_else(|_| "farf".to_string());
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

        let test_delegate_stake = test_commands.clone().get_matches_from(vec![
            "test",
            "delegate-stake",
            &keypair_file,
            &pubkey_string,
            "42",
        ]);
        assert_eq!(
            parse_command(&pubkey, &test_delegate_stake).unwrap(),
            WalletCommand::DelegateStake(keypair, pubkey, 42, false)
        );

        let keypair = read_keypair(&keypair_file).unwrap();
        let test_delegate_stake = test_commands.clone().get_matches_from(vec![
            "test",
            "delegate-stake",
            "--force",
            &keypair_file,
            &pubkey_string,
            "42",
        ]);
        assert_eq!(
            parse_command(&pubkey, &test_delegate_stake).unwrap(),
            WalletCommand::DelegateStake(keypair, pubkey, 42, true)
        );

        // Test WithdrawStake Subcommand
        let test_withdraw_stake = test_commands.clone().get_matches_from(vec![
            "test",
            "withdraw-stake",
            &keypair_file,
            &pubkey_string,
            "42",
        ]);
        let keypair = read_keypair(&keypair_file).unwrap();
        assert_eq!(
            parse_command(&pubkey, &test_withdraw_stake).unwrap(),
            WalletCommand::WithdrawStake(keypair, pubkey, 42)
        );

        // Test DeactivateStake Subcommand
        let keypair_file = make_tmp_path("keypair_file");
        gen_keypair_file(&keypair_file).unwrap();
        let keypair = read_keypair(&keypair_file).unwrap();
        let test_deactivate_stake = test_commands.clone().get_matches_from(vec![
            "test",
            "deactivate-stake",
            &keypair_file,
            &pubkey_string,
        ]);
        assert_eq!(
            parse_command(&pubkey, &test_deactivate_stake).unwrap(),
            WalletCommand::DeactivateStake(keypair, pubkey)
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
        let new_authorized_voter_pubkey = Pubkey::new_rand();
        config.command =
            WalletCommand::AuthorizeVoter(bob_pubkey, bob_keypair, new_authorized_voter_pubkey);
        let signature = process_command(&config);
        assert_eq!(signature.unwrap(), SIGNATURE.to_string());

        // TODO: Need to add mock GetAccountInfo to mock_rpc_client_request.rs to re-enable the
        // DeactivateStake test.
        /*
        let bob_keypair = Keypair::new();
        let vote_pubkey = Pubkey::new_rand();
        config.command = WalletCommand::DelegateStake(bob_keypair.into(), vote_pubkey, 100, true);
        let signature = process_command(&config);
        assert_eq!(signature.unwrap(), SIGNATURE.to_string());
        */

        let bob_keypair = Keypair::new();
        let to_pubkey = Pubkey::new_rand();
        config.command = WalletCommand::WithdrawStake(bob_keypair.into(), to_pubkey, 100);
        let signature = process_command(&config);
        assert_eq!(signature.unwrap(), SIGNATURE.to_string());

        let bob_keypair = Keypair::new();
        let vote_pubkey = Pubkey::new_rand();
        config.command = WalletCommand::DeactivateStake(bob_keypair.into(), vote_pubkey);
        let signature = process_command(&config);
        assert_eq!(signature.unwrap(), SIGNATURE.to_string());

        config.command = WalletCommand::GetSlot;
        assert_eq!(process_command(&config).unwrap(), "0");

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
        config.command = WalletCommand::Airdrop {
            drone_host: None,
            drone_port: 1234,
            lamports: 50,
        };
        assert!(process_command(&config).is_ok());

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

        config.command = WalletCommand::Airdrop {
            drone_host: None,
            drone_port: 1234,
            lamports: 50,
        };
        assert!(process_command(&config).is_err());

        config.command = WalletCommand::Balance(config.keypair.pubkey());
        assert!(process_command(&config).is_err());

        config.command = WalletCommand::CreateVoteAccount(bob_pubkey, node_pubkey, 0, 10);
        assert!(process_command(&config).is_err());

        config.command = WalletCommand::AuthorizeVoter(bob_pubkey, Keypair::new(), bob_pubkey);
        assert!(process_command(&config).is_err());

        config.command = WalletCommand::GetSlot;
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
        config.rpc_client = Some(RpcClient::new_mock("deploy_succeeds".to_string()));

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

        // Failure case
        config.command = WalletCommand::Deploy("bad/file/location.so".to_string());
        assert!(process_command(&config).is_err());
    }
}
