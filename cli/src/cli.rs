use crate::{
    cluster_query::*, display::println_name_value, input_parsers::*, input_validators::*, stake::*,
    storage::*, validator_info::*, vote::*,
};
use chrono::prelude::*;
use clap::{App, AppSettings, Arg, ArgMatches, SubCommand};
use log::*;
use num_traits::FromPrimitive;
use serde_json::{self, json, Value};
use solana_budget_api::budget_instruction::{self, BudgetError};
use solana_client::{client_error::ClientError, rpc_client::RpcClient};
#[cfg(not(test))]
use solana_drone::drone::request_airdrop_transaction;
#[cfg(test)]
use solana_drone::drone_mock::request_airdrop_transaction;
use solana_sdk::{
    bpf_loader,
    fee_calculator::FeeCalculator,
    hash::Hash,
    instruction::InstructionError,
    instruction_processor_utils::DecodeError,
    loader_instruction,
    message::Message,
    native_token::lamports_to_sol,
    pubkey::Pubkey,
    signature::{Keypair, KeypairUtil, Signature},
    system_instruction::SystemError,
    system_transaction,
    transaction::{Transaction, TransactionError},
};
use solana_stake_api::stake_state::{Lockup, StakeAuthorize};
use solana_storage_api::storage_instruction::StorageAccountType;
use solana_vote_api::vote_state::VoteAuthorize;
use std::{
    fs::File,
    io::{Read, Write},
    net::{IpAddr, SocketAddr},
    thread::sleep,
    time::Duration,
    {error, fmt},
};

const USERDATA_CHUNK_SIZE: usize = 229; // Keep program chunks under PACKET_DATA_SIZE

#[derive(Debug, PartialEq)]
#[allow(clippy::large_enum_variant)]
pub enum CliCommand {
    // Cluster Query Commands
    ClusterVersion,
    Fees,
    GetEpochInfo,
    GetGenesisBlockhash,
    GetSlot,
    GetTransactionCount,
    Ping {
        interval: Duration,
        count: Option<u64>,
        timeout: Duration,
    },
    ShowValidators {
        use_lamports_unit: bool,
    },
    // Program Deployment
    Deploy(String),
    // Stake Commands
    CreateStakeAccount {
        stake_account_pubkey: Pubkey,
        staker: Option<Pubkey>,
        withdrawer: Option<Pubkey>,
        lockup: Lockup,
        lamports: u64,
    },
    DeactivateStake(Pubkey),
    DelegateStake(Pubkey, Pubkey, bool),
    RedeemVoteCredits(Pubkey, Pubkey),
    ShowStakeHistory {
        use_lamports_unit: bool,
    },
    ShowStakeAccount {
        pubkey: Pubkey,
        use_lamports_unit: bool,
    },
    StakeAuthorize(Pubkey, Pubkey, StakeAuthorize),
    WithdrawStake(Pubkey, Pubkey, u64),
    // Storage Commands
    CreateStorageAccount {
        account_owner: Pubkey,
        storage_account_pubkey: Pubkey,
        account_type: StorageAccountType,
    },
    ClaimStorageReward {
        node_account_pubkey: Pubkey,
        storage_account_pubkey: Pubkey,
    },
    ShowStorageAccount(Pubkey),
    // Validator Info Commands
    GetValidatorInfo(Option<Pubkey>),
    SetValidatorInfo {
        validator_info: Value,
        force_keybase: bool,
        info_pubkey: Option<Pubkey>,
    },
    // Vote Commands
    CreateVoteAccount {
        vote_account_pubkey: Pubkey,
        node_pubkey: Pubkey,
        authorized_voter: Option<Pubkey>,
        authorized_withdrawer: Option<Pubkey>,
        commission: u8,
    },
    ShowVoteAccount {
        pubkey: Pubkey,
        use_lamports_unit: bool,
    },
    Uptime {
        pubkey: Pubkey,
        aggregate: bool,
        span: Option<u64>,
    },
    VoteAuthorize(Pubkey, Pubkey, VoteAuthorize),
    // Wallet Commands
    Address,
    Airdrop {
        drone_host: Option<IpAddr>,
        drone_port: u16,
        lamports: u64,
        use_lamports_unit: bool,
    },
    Balance {
        pubkey: Option<Pubkey>,
        use_lamports_unit: bool,
    },
    Cancel(Pubkey),
    Confirm(Signature),
    Pay {
        lamports: u64,
        to: Pubkey,
        timestamp: Option<DateTime<Utc>>,
        timestamp_pubkey: Option<Pubkey>,
        witnesses: Option<Vec<Pubkey>>,
        cancelable: bool,
    },
    ShowAccount {
        pubkey: Pubkey,
        output_file: Option<String>,
        use_lamports_unit: bool,
    },
    TimeElapsed(Pubkey, Pubkey, DateTime<Utc>), // TimeElapsed(to, process_id, timestamp)
    Witness(Pubkey, Pubkey),                    // Witness(to, process_id)
}

#[derive(Debug, PartialEq)]
pub struct CliCommandInfo {
    pub command: CliCommand,
    pub require_keypair: bool,
}

#[derive(Debug, Clone)]
pub enum CliError {
    BadParameter(String),
    CommandNotRecognized(String),
    InsufficientFundsForFee,
    DynamicProgramError(String),
    RpcRequestError(String),
    KeypairFileNotFound(String),
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid")
    }
}

impl error::Error for CliError {
    fn description(&self) -> &str {
        "invalid"
    }

    fn cause(&self) -> Option<&dyn error::Error> {
        // Generic error, underlying cause isn't tracked.
        None
    }
}

pub struct CliConfig {
    pub command: CliCommand,
    pub json_rpc_url: String,
    pub keypair: Keypair,
    pub keypair_path: Option<String>,
    pub rpc_client: Option<RpcClient>,
}

impl Default for CliConfig {
    fn default() -> CliConfig {
        let mut keypair_path = dirs::home_dir().expect("home directory");
        keypair_path.extend(&[".config", "solana", "id.json"]);

        CliConfig {
            command: CliCommand::Balance {
                pubkey: Some(Pubkey::default()),
                use_lamports_unit: false,
            },
            json_rpc_url: "http://127.0.0.1:8899".to_string(),
            keypair: Keypair::new(),
            keypair_path: Some(keypair_path.to_str().unwrap().to_string()),
            rpc_client: None,
        }
    }
}

pub fn parse_command(matches: &ArgMatches<'_>) -> Result<CliCommandInfo, Box<dyn error::Error>> {
    let response = match matches.subcommand() {
        // Cluster Query Commands
        ("cluster-version", Some(_matches)) => Ok(CliCommandInfo {
            command: CliCommand::ClusterVersion,
            require_keypair: false,
        }),
        ("fees", Some(_matches)) => Ok(CliCommandInfo {
            command: CliCommand::Fees,
            require_keypair: false,
        }),
        ("get-epoch-info", Some(_matches)) => Ok(CliCommandInfo {
            command: CliCommand::GetEpochInfo,
            require_keypair: false,
        }),
        ("get-genesis-blockhash", Some(_matches)) => Ok(CliCommandInfo {
            command: CliCommand::GetGenesisBlockhash,
            require_keypair: false,
        }),
        ("get-slot", Some(_matches)) => Ok(CliCommandInfo {
            command: CliCommand::GetSlot,
            require_keypair: false,
        }),
        ("get-transaction-count", Some(_matches)) => Ok(CliCommandInfo {
            command: CliCommand::GetTransactionCount,
            require_keypair: false,
        }),
        ("ping", Some(matches)) => parse_cluster_ping(matches),
        ("show-validators", Some(matches)) => parse_show_validators(matches),
        // Program Deployment
        ("deploy", Some(matches)) => Ok(CliCommandInfo {
            command: CliCommand::Deploy(matches.value_of("program_location").unwrap().to_string()),
            require_keypair: true,
        }),
        // Stake Commands
        ("create-stake-account", Some(matches)) => parse_stake_create_account(matches),
        ("delegate-stake", Some(matches)) => parse_stake_delegate_stake(matches),
        ("withdraw-stake", Some(matches)) => parse_stake_withdraw_stake(matches),
        ("deactivate-stake", Some(matches)) => parse_stake_deactivate_stake(matches),
        ("stake-authorize-staker", Some(matches)) => {
            parse_stake_authorize(matches, StakeAuthorize::Staker)
        }
        ("stake-authorize-withdrawer", Some(matches)) => {
            parse_stake_authorize(matches, StakeAuthorize::Withdrawer)
        }
        ("redeem-vote-credits", Some(matches)) => parse_redeem_vote_credits(matches),
        ("show-stake-account", Some(matches)) => parse_show_stake_account(matches),
        ("show-stake-history", Some(matches)) => parse_show_stake_history(matches),
        // Storage Commands
        ("create-archiver-storage-account", Some(matches)) => {
            parse_storage_create_archiver_account(matches)
        }
        ("create-validator-storage-account", Some(matches)) => {
            parse_storage_create_validator_account(matches)
        }
        ("claim-storage-reward", Some(matches)) => parse_storage_claim_reward(matches),
        ("show-storage-account", Some(matches)) => parse_storage_get_account_command(matches),
        // Validator Info Commands
        ("validator-info", Some(matches)) => match matches.subcommand() {
            ("publish", Some(matches)) => parse_validator_info_command(matches),
            ("get", Some(matches)) => parse_get_validator_info_command(matches),
            ("", None) => {
                eprintln!("{}", matches.usage());
                Err(CliError::CommandNotRecognized(
                    "no validator-info subcommand given".to_string(),
                ))
            }
            _ => unreachable!(),
        },
        // Vote Commands
        ("create-vote-account", Some(matches)) => parse_vote_create_account(matches),
        ("vote-authorize-voter", Some(matches)) => {
            parse_vote_authorize(matches, VoteAuthorize::Voter)
        }
        ("vote-authorize-withdrawer", Some(matches)) => {
            parse_vote_authorize(matches, VoteAuthorize::Withdrawer)
        }
        ("show-vote-account", Some(matches)) => parse_vote_get_account_command(matches),
        ("uptime", Some(matches)) => parse_vote_uptime_command(matches),
        // Wallet Commands
        ("address", Some(_matches)) => Ok(CliCommandInfo {
            command: CliCommand::Address,
            require_keypair: true,
        }),
        ("airdrop", Some(matches)) => {
            let drone_port = matches
                .value_of("drone_port")
                .unwrap()
                .parse()
                .or_else(|err| {
                    Err(CliError::BadParameter(format!(
                        "Invalid drone port: {:?}",
                        err
                    )))
                })?;

            let drone_host = if let Some(drone_host) = matches.value_of("drone_host") {
                Some(solana_netutil::parse_host(drone_host).or_else(|err| {
                    Err(CliError::BadParameter(format!(
                        "Invalid drone host: {:?}",
                        err
                    )))
                })?)
            } else {
                None
            };
            let lamports = amount_of(matches, "amount", "unit").expect("Invalid amount");
            let use_lamports_unit = matches.value_of("unit").is_some()
                && matches.value_of("unit").unwrap() == "lamports";
            Ok(CliCommandInfo {
                command: CliCommand::Airdrop {
                    drone_host,
                    drone_port,
                    lamports,
                    use_lamports_unit,
                },
                require_keypair: true,
            })
        }
        ("balance", Some(matches)) => {
            let pubkey = pubkey_of(&matches, "pubkey");
            println!("{:?}", pubkey);
            Ok(CliCommandInfo {
                command: CliCommand::Balance {
                    pubkey,
                    use_lamports_unit: matches.is_present("lamports"),
                },
                require_keypair: pubkey.is_none(),
            })
        }
        ("cancel", Some(matches)) => {
            let process_id = value_of(matches, "process_id").unwrap();
            Ok(CliCommandInfo {
                command: CliCommand::Cancel(process_id),
                require_keypair: true,
            })
        }
        ("confirm", Some(matches)) => match matches.value_of("signature").unwrap().parse() {
            Ok(signature) => Ok(CliCommandInfo {
                command: CliCommand::Confirm(signature),
                require_keypair: false,
            }),
            _ => {
                eprintln!("{}", matches.usage());
                Err(CliError::BadParameter("Invalid signature".to_string()))
            }
        },
        ("pay", Some(matches)) => {
            let lamports = amount_of(matches, "amount", "unit").expect("Invalid amount");
            let to = value_of(&matches, "to").unwrap();
            let timestamp = if matches.is_present("timestamp") {
                // Parse input for serde_json
                let date_string = if !matches.value_of("timestamp").unwrap().contains('Z') {
                    format!("\"{}Z\"", matches.value_of("timestamp").unwrap())
                } else {
                    format!("\"{}\"", matches.value_of("timestamp").unwrap())
                };
                Some(serde_json::from_str(&date_string)?)
            } else {
                None
            };
            let timestamp_pubkey = value_of(&matches, "timestamp_pubkey");
            let witnesses = values_of(&matches, "witness");
            let cancelable = matches.is_present("cancelable");

            Ok(CliCommandInfo {
                command: CliCommand::Pay {
                    lamports,
                    to,
                    timestamp,
                    timestamp_pubkey,
                    witnesses,
                    cancelable,
                },
                require_keypair: true,
            })
        }
        ("show-account", Some(matches)) => {
            let account_pubkey = pubkey_of(matches, "account_pubkey").unwrap();
            let output_file = matches.value_of("output_file");
            let use_lamports_unit = matches.is_present("lamports");
            Ok(CliCommandInfo {
                command: CliCommand::ShowAccount {
                    pubkey: account_pubkey,
                    output_file: output_file.map(ToString::to_string),
                    use_lamports_unit,
                },
                require_keypair: false,
            })
        }
        ("send-signature", Some(matches)) => {
            let to = value_of(&matches, "to").unwrap();
            let process_id = value_of(&matches, "process_id").unwrap();
            Ok(CliCommandInfo {
                command: CliCommand::Witness(to, process_id),
                require_keypair: true,
            })
        }
        ("send-timestamp", Some(matches)) => {
            let to = value_of(&matches, "to").unwrap();
            let process_id = value_of(&matches, "process_id").unwrap();
            let dt = if matches.is_present("datetime") {
                // Parse input for serde_json
                let date_string = if !matches.value_of("datetime").unwrap().contains('Z') {
                    format!("\"{}Z\"", matches.value_of("datetime").unwrap())
                } else {
                    format!("\"{}\"", matches.value_of("datetime").unwrap())
                };
                serde_json::from_str(&date_string)?
            } else {
                Utc::now()
            };
            Ok(CliCommandInfo {
                command: CliCommand::TimeElapsed(to, process_id, dt),
                require_keypair: true,
            })
        }
        ("", None) => {
            eprintln!("{}", matches.usage());
            Err(CliError::CommandNotRecognized(
                "no subcommand given".to_string(),
            ))
        }
        _ => unreachable!(),
    }?;
    Ok(response)
}

pub type ProcessResult = Result<String, Box<dyn std::error::Error>>;

pub fn check_account_for_fee(
    rpc_client: &RpcClient,
    config: &CliConfig,
    fee_calculator: &FeeCalculator,
    message: &Message,
) -> Result<(), Box<dyn error::Error>> {
    check_account_for_multiple_fees(rpc_client, config, fee_calculator, &[message])
}

fn check_account_for_multiple_fees(
    rpc_client: &RpcClient,
    config: &CliConfig,
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
    Err(CliError::InsufficientFundsForFee.into())
}

pub fn check_unique_pubkeys(
    pubkey0: (&Pubkey, String),
    pubkey1: (&Pubkey, String),
) -> Result<(), CliError> {
    if pubkey0.0 == pubkey1.0 {
        Err(CliError::BadParameter(format!(
            "Identical pubkeys found: `{}` and `{}` must be unique",
            pubkey0.1, pubkey1.1
        )))
    } else {
        Ok(())
    }
}

fn process_airdrop(
    rpc_client: &RpcClient,
    config: &CliConfig,
    drone_addr: &SocketAddr,
    lamports: u64,
    use_lamports_unit: bool,
) -> ProcessResult {
    println!(
        "Requesting airdrop of {} from {}",
        build_balance_message(lamports, use_lamports_unit, true),
        drone_addr
    );
    let previous_balance = match rpc_client.retry_get_balance(&config.keypair.pubkey(), 5)? {
        Some(lamports) => lamports,
        None => {
            return Err(CliError::RpcRequestError(
                "Received result of an unexpected type".to_string(),
            )
            .into())
        }
    };

    request_and_confirm_airdrop(&rpc_client, drone_addr, &config.keypair.pubkey(), lamports)?;

    let current_balance = rpc_client
        .retry_get_balance(&config.keypair.pubkey(), 5)?
        .unwrap_or(previous_balance);

    Ok(build_balance_message(
        current_balance,
        use_lamports_unit,
        true,
    ))
}

fn process_balance(
    rpc_client: &RpcClient,
    config: &CliConfig,
    pubkey: &Option<Pubkey>,
    use_lamports_unit: bool,
) -> ProcessResult {
    let pubkey = pubkey.unwrap_or(config.keypair.pubkey());
    let balance = rpc_client.retry_get_balance(&pubkey, 5)?;
    match balance {
        Some(lamports) => Ok(build_balance_message(lamports, use_lamports_unit, true)),
        None => Err(
            CliError::RpcRequestError("Received result of an unexpected type".to_string()).into(),
        ),
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
        Err(err) => Err(CliError::RpcRequestError(format!("Unable to confirm: {:?}", err)).into()),
    }
}

fn process_show_account(
    rpc_client: &RpcClient,
    _config: &CliConfig,
    account_pubkey: &Pubkey,
    output_file: &Option<String>,
    use_lamports_unit: bool,
) -> ProcessResult {
    let account = rpc_client.get_account(account_pubkey)?;

    println!();
    println_name_value("Public Key:", &account_pubkey.to_string());
    println_name_value(
        "Balance:",
        &build_balance_message(account.lamports, use_lamports_unit, true),
    );
    println_name_value("Owner:", &account.owner.to_string());
    println_name_value("Executable:", &account.executable.to_string());

    if let Some(output_file) = output_file {
        let mut f = File::create(output_file)?;
        f.write_all(&account.data)?;
        println!();
        println!("Wrote account data to {}", output_file);
    } else if !account.data.is_empty() {
        use pretty_hex::*;
        println!("{:?}", account.data.hex_dump());
    }

    Ok("".to_string())
}

fn process_deploy(
    rpc_client: &RpcClient,
    config: &CliConfig,
    program_location: &str,
) -> ProcessResult {
    let program_id = Keypair::new();
    let mut file = File::open(program_location).map_err(|err| {
        CliError::DynamicProgramError(format!("Unable to open program file: {}", err).to_string())
    })?;
    let mut program_data = Vec::new();
    file.read_to_end(&mut program_data).map_err(|err| {
        CliError::DynamicProgramError(format!("Unable to read program file: {}", err).to_string())
    })?;

    // Build transactions to calculate fees
    let mut messages: Vec<&Message> = Vec::new();
    let (blockhash, fee_calculator) = rpc_client.get_recent_blockhash()?;
    let minimum_balance = rpc_client.get_minimum_balance_for_rent_exemption(program_data.len())?;
    let mut create_account_tx = system_transaction::create_account(
        &config.keypair,
        &program_id.pubkey(),
        blockhash,
        minimum_balance,
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
    log_instruction_custom_error::<SystemError>(result)
        .map_err(|_| CliError::DynamicProgramError("Program allocate space failed".to_string()))?;

    trace!("Writing program data");
    rpc_client.send_and_confirm_transactions(write_transactions, &signers)?;

    trace!("Finalizing program account");
    rpc_client
        .send_and_confirm_transaction(&mut finalize_tx, &signers)
        .map_err(|_| {
            CliError::DynamicProgramError("Program finalize transaction failed".to_string())
        })?;

    Ok(json!({
        "programId": format!("{}", program_id.pubkey()),
    })
    .to_string())
}

fn process_pay(
    rpc_client: &RpcClient,
    config: &CliConfig,
    lamports: u64,
    to: &Pubkey,
    timestamp: Option<DateTime<Utc>>,
    timestamp_pubkey: Option<Pubkey>,
    witnesses: &Option<Vec<Pubkey>>,
    cancelable: bool,
) -> ProcessResult {
    check_unique_pubkeys(
        (&config.keypair.pubkey(), "cli keypair".to_string()),
        (to, "to".to_string()),
    )?;
    let (blockhash, fee_calculator) = rpc_client.get_recent_blockhash()?;

    let cancelable = if cancelable {
        Some(config.keypair.pubkey())
    } else {
        None
    };

    if timestamp == None && *witnesses == None {
        let mut tx = system_transaction::transfer(&config.keypair, to, lamports, blockhash);
        check_account_for_fee(rpc_client, config, &fee_calculator, &tx.message)?;
        let result = rpc_client.send_and_confirm_transaction(&mut tx, &[&config.keypair]);
        log_instruction_custom_error::<SystemError>(result)
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
            return Err(CliError::BadParameter(
                "Could not parse required signature pubkey(s)".to_string(),
            )
            .into());
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

fn process_cancel(rpc_client: &RpcClient, config: &CliConfig, pubkey: &Pubkey) -> ProcessResult {
    let (blockhash, fee_calculator) = rpc_client.get_recent_blockhash()?;
    let ix = budget_instruction::apply_signature(
        &config.keypair.pubkey(),
        pubkey,
        &config.keypair.pubkey(),
    );
    let mut tx = Transaction::new_signed_instructions(&[&config.keypair], vec![ix], blockhash);
    check_account_for_fee(rpc_client, config, &fee_calculator, &tx.message)?;
    let result = rpc_client.send_and_confirm_transaction(&mut tx, &[&config.keypair]);
    log_instruction_custom_error::<BudgetError>(result)
}

fn process_time_elapsed(
    rpc_client: &RpcClient,
    config: &CliConfig,
    to: &Pubkey,
    pubkey: &Pubkey,
    dt: DateTime<Utc>,
) -> ProcessResult {
    let (blockhash, fee_calculator) = rpc_client.get_recent_blockhash()?;

    let ix = budget_instruction::apply_timestamp(&config.keypair.pubkey(), pubkey, to, dt);
    let mut tx = Transaction::new_signed_instructions(&[&config.keypair], vec![ix], blockhash);
    check_account_for_fee(rpc_client, config, &fee_calculator, &tx.message)?;
    let result = rpc_client.send_and_confirm_transaction(&mut tx, &[&config.keypair]);
    log_instruction_custom_error::<BudgetError>(result)
}

fn process_witness(
    rpc_client: &RpcClient,
    config: &CliConfig,
    to: &Pubkey,
    pubkey: &Pubkey,
) -> ProcessResult {
    let (blockhash, fee_calculator) = rpc_client.get_recent_blockhash()?;

    let ix = budget_instruction::apply_signature(&config.keypair.pubkey(), pubkey, to);
    let mut tx = Transaction::new_signed_instructions(&[&config.keypair], vec![ix], blockhash);
    check_account_for_fee(rpc_client, config, &fee_calculator, &tx.message)?;
    let result = rpc_client.send_and_confirm_transaction(&mut tx, &[&config.keypair]);
    log_instruction_custom_error::<BudgetError>(result)
}

pub fn process_command(config: &CliConfig) -> ProcessResult {
    if let Some(keypair_path) = &config.keypair_path {
        println_name_value("Keypair:", keypair_path);
    }
    if let CliCommand::Address = config.command {
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
        // Cluster Query Commands

        // Return software version of solana-cli and cluster entrypoint node
        CliCommand::ClusterVersion => process_cluster_version(&rpc_client, config),
        CliCommand::Fees => process_fees(&rpc_client),
        CliCommand::GetGenesisBlockhash => process_get_genesis_blockhash(&rpc_client),
        CliCommand::GetSlot => process_get_slot(&rpc_client),
        CliCommand::GetEpochInfo => process_get_epoch_info(&rpc_client),
        CliCommand::GetTransactionCount => process_get_transaction_count(&rpc_client),
        CliCommand::Ping {
            interval,
            count,
            timeout,
        } => process_ping(&rpc_client, config, interval, count, timeout),
        CliCommand::ShowValidators { use_lamports_unit } => {
            process_show_validators(&rpc_client, *use_lamports_unit)
        }

        // Program Deployment

        // Deploy a custom program to the chain
        CliCommand::Deploy(ref program_location) => {
            process_deploy(&rpc_client, config, program_location)
        }

        // Stake Commands

        // Create stake account
        CliCommand::CreateStakeAccount {
            stake_account_pubkey,
            staker,
            withdrawer,
            lockup,
            lamports,
        } => process_create_stake_account(
            &rpc_client,
            config,
            &stake_account_pubkey,
            staker,
            withdrawer,
            lockup,
            *lamports,
        ),
        // Deactivate stake account
        CliCommand::DeactivateStake(stake_account_pubkey) => {
            process_deactivate_stake_account(&rpc_client, config, &stake_account_pubkey)
        }
        CliCommand::DelegateStake(stake_account_pubkey, vote_account_pubkey, force) => {
            process_delegate_stake(
                &rpc_client,
                config,
                &stake_account_pubkey,
                &vote_account_pubkey,
                *force,
            )
        }
        CliCommand::RedeemVoteCredits(stake_account_pubkey, vote_account_pubkey) => {
            process_redeem_vote_credits(
                &rpc_client,
                config,
                &stake_account_pubkey,
                &vote_account_pubkey,
            )
        }
        CliCommand::ShowStakeAccount {
            pubkey: stake_account_pubkey,
            use_lamports_unit,
        } => process_show_stake_account(
            &rpc_client,
            config,
            &stake_account_pubkey,
            *use_lamports_unit,
        ),
        CliCommand::ShowStakeHistory { use_lamports_unit } => {
            process_show_stake_history(&rpc_client, config, *use_lamports_unit)
        }
        CliCommand::StakeAuthorize(
            stake_account_pubkey,
            new_authorized_pubkey,
            stake_authorize,
        ) => process_stake_authorize(
            &rpc_client,
            config,
            &stake_account_pubkey,
            &new_authorized_pubkey,
            *stake_authorize,
        ),

        CliCommand::WithdrawStake(stake_account_pubkey, destination_account_pubkey, lamports) => {
            process_withdraw_stake(
                &rpc_client,
                config,
                &stake_account_pubkey,
                &destination_account_pubkey,
                *lamports,
            )
        }

        // Storage Commands

        // Create storage account
        CliCommand::CreateStorageAccount {
            account_owner,
            storage_account_pubkey,
            account_type,
        } => process_create_storage_account(
            &rpc_client,
            config,
            &account_owner,
            &storage_account_pubkey,
            *account_type,
        ),
        CliCommand::ClaimStorageReward {
            node_account_pubkey,
            storage_account_pubkey,
        } => process_claim_storage_reward(
            &rpc_client,
            config,
            node_account_pubkey,
            &storage_account_pubkey,
        ),
        CliCommand::ShowStorageAccount(storage_account_pubkey) => {
            process_show_storage_account(&rpc_client, config, &storage_account_pubkey)
        }

        // Validator Info Commands

        // Return all or single validator info
        CliCommand::GetValidatorInfo(info_pubkey) => {
            process_get_validator_info(&rpc_client, *info_pubkey)
        }
        // Publish validator info
        CliCommand::SetValidatorInfo {
            validator_info,
            force_keybase,
            info_pubkey,
        } => process_set_validator_info(
            &rpc_client,
            config,
            &validator_info,
            *force_keybase,
            *info_pubkey,
        ),

        // Vote Commands

        // Create vote account
        CliCommand::CreateVoteAccount {
            vote_account_pubkey,
            node_pubkey,
            authorized_voter,
            authorized_withdrawer,
            commission,
        } => process_create_vote_account(
            &rpc_client,
            config,
            &vote_account_pubkey,
            &node_pubkey,
            authorized_voter,
            authorized_withdrawer,
            *commission,
        ),
        CliCommand::ShowVoteAccount {
            pubkey: vote_account_pubkey,
            use_lamports_unit,
        } => process_show_vote_account(
            &rpc_client,
            config,
            &vote_account_pubkey,
            *use_lamports_unit,
        ),
        CliCommand::VoteAuthorize(vote_account_pubkey, new_authorized_pubkey, vote_authorize) => {
            process_vote_authorize(
                &rpc_client,
                config,
                &vote_account_pubkey,
                &new_authorized_pubkey,
                *vote_authorize,
            )
        }
        CliCommand::Uptime {
            pubkey: vote_account_pubkey,
            aggregate,
            span,
        } => process_uptime(&rpc_client, config, &vote_account_pubkey, *aggregate, *span),

        // Wallet Commands

        // Get address of this client
        CliCommand::Address => unreachable!(),
        // Request an airdrop from Solana Drone;
        CliCommand::Airdrop {
            drone_host,
            drone_port,
            lamports,
            use_lamports_unit,
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

            process_airdrop(
                &rpc_client,
                config,
                &drone_addr,
                *lamports,
                *use_lamports_unit,
            )
        }
        // Check client balance
        CliCommand::Balance {
            pubkey,
            use_lamports_unit,
        } => process_balance(&rpc_client, config, &pubkey, *use_lamports_unit),
        // Cancel a contract by contract Pubkey
        CliCommand::Cancel(pubkey) => process_cancel(&rpc_client, config, &pubkey),
        // Confirm the last client transaction by signature
        CliCommand::Confirm(signature) => process_confirm(&rpc_client, signature),
        // If client has positive balance, pay lamports to another address
        CliCommand::Pay {
            lamports,
            to,
            timestamp,
            timestamp_pubkey,
            ref witnesses,
            cancelable,
        } => process_pay(
            &rpc_client,
            config,
            *lamports,
            &to,
            *timestamp,
            *timestamp_pubkey,
            witnesses,
            *cancelable,
        ),
        CliCommand::ShowAccount {
            pubkey,
            output_file,
            use_lamports_unit,
        } => process_show_account(
            &rpc_client,
            config,
            &pubkey,
            &output_file,
            *use_lamports_unit,
        ),
        // Apply time elapsed to contract
        CliCommand::TimeElapsed(to, pubkey, dt) => {
            process_time_elapsed(&rpc_client, config, &to, &pubkey, *dt)
        }
        // Apply witness signature to contract
        CliCommand::Witness(to, pubkey) => process_witness(&rpc_client, config, &to, &pubkey),
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
) -> ProcessResult {
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
    log_instruction_custom_error::<SystemError>(result)
}

pub fn log_instruction_custom_error<E>(result: Result<String, ClientError>) -> ProcessResult
where
    E: 'static + std::error::Error + DecodeError<E> + FromPrimitive,
{
    match result {
        Err(err) => {
            if let ClientError::TransactionError(TransactionError::InstructionError(
                _,
                InstructionError::CustomError(code),
            )) = err
            {
                if let Some(specific_error) = E::decode_custom_error_to_enum(code) {
                    error!("{}::{:?}", E::type_of(), specific_error);
                    return Err(specific_error.into());
                }
            }
            error!("{:?}", err);
            Err(err.into())
        }
        Ok(sig) => Ok(sig),
    }
}

pub(crate) fn build_balance_message(
    lamports: u64,
    use_lamports_unit: bool,
    show_unit: bool,
) -> String {
    if use_lamports_unit {
        let ess = if lamports == 1 { "" } else { "s" };
        let unit = if show_unit {
            format!(" lamport{}", ess)
        } else {
            "".to_string()
        };
        format!("{:?}{}", lamports, unit)
    } else {
        let sol = lamports_to_sol(lamports);
        let sol_str = format!("{:.9}", sol);
        let pretty_sol = sol_str.trim_end_matches('0').trim_end_matches('.');
        let unit = if show_unit { " SOL" } else { "" };
        format!("{}{}", pretty_sol, unit)
    }
}

pub fn app<'ab, 'v>(name: &str, about: &'ab str, version: &'v str) -> App<'ab, 'v> {
    App::new(name)
        .about(about)
        .version(version)
        .setting(AppSettings::SubcommandRequiredElseHelp)
        .subcommand(SubCommand::with_name("address").about("Get your public key"))
        .cluster_query_subcommands()
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
        .stake_subcommands()
        .storage_subcommands()
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
                    Arg::with_name("amount")
                        .index(1)
                        .value_name("AMOUNT")
                        .takes_value(true)
                        .required(true)
                        .help("The airdrop amount to request (default unit SOL)"),
                )
                .arg(
                    Arg::with_name("unit")
                        .index(2)
                        .value_name("UNIT")
                        .takes_value(true)
                        .possible_values(&["SOL", "lamports"])
                        .help("Specify unit to use for request and balance display"),
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
                )
                .arg(
                    Arg::with_name("lamports")
                        .long("lamports")
                        .takes_value(false)
                        .help("Display balance in lamports instead of SOL"),
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
                    Arg::with_name("amount")
                        .index(2)
                        .value_name("AMOUNT")
                        .takes_value(true)
                        .required(true)
                        .help("The amount to send (default unit SOL)"),
                )
                .arg(
                    Arg::with_name("unit")
                        .index(3)
                        .value_name("UNIT")
                        .takes_value(true)
                        .possible_values(&["SOL", "lamports"])
                        .help("Specify unit to use for request"),
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
                )
                .arg(
                    Arg::with_name("lamports")
                        .long("lamports")
                        .takes_value(false)
                        .help("Display balance in lamports instead of SOL"),
                ),
        )
        .validator_info_subcommands()
        .vote_subcommands()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use solana_client::mock_rpc_client_request::SIGNATURE;
    use solana_sdk::{
        signature::{gen_keypair_file, read_keypair_file},
        transaction::TransactionError,
    };
    use std::path::PathBuf;

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

    #[test]
    #[should_panic]
    fn test_bad_amount() {
        let test_commands = app("test", "desc", "version");
        let test_bad_airdrop = test_commands.get_matches_from(vec!["test", "airdrop", "notint"]);
        let _ignored = parse_command(&test_bad_airdrop).unwrap();
    }

    #[test]
    fn test_cli_parse_command() {
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
            .get_matches_from(vec!["test", "airdrop", "50", "lamports"]);
        assert_eq!(
            parse_command(&test_airdrop).unwrap(),
            CliCommandInfo {
                command: CliCommand::Airdrop {
                    drone_host: None,
                    drone_port: solana_drone::drone::DRONE_PORT,
                    lamports: 50,
                    use_lamports_unit: true,
                },
                require_keypair: true,
            }
        );

        // Test Balance Subcommand, incl pubkey and keypair-file inputs
        let keypair_file = make_tmp_path("keypair_file");
        gen_keypair_file(&keypair_file).unwrap();
        let keypair = read_keypair_file(&keypair_file).unwrap();
        let test_balance = test_commands.clone().get_matches_from(vec![
            "test",
            "balance",
            &keypair.pubkey().to_string(),
        ]);
        assert_eq!(
            parse_command(&test_balance).unwrap(),
            CliCommandInfo {
                command: CliCommand::Balance {
                    pubkey: Some(keypair.pubkey()),
                    use_lamports_unit: false
                },
                require_keypair: false
            }
        );
        let test_balance = test_commands.clone().get_matches_from(vec![
            "test",
            "balance",
            &keypair_file,
            "--lamports",
        ]);
        assert_eq!(
            parse_command(&test_balance).unwrap(),
            CliCommandInfo {
                command: CliCommand::Balance {
                    pubkey: Some(keypair.pubkey()),
                    use_lamports_unit: true
                },
                require_keypair: false
            }
        );
        let test_balance =
            test_commands
                .clone()
                .get_matches_from(vec!["test", "balance", "--lamports"]);
        assert_eq!(
            parse_command(&test_balance).unwrap(),
            CliCommandInfo {
                command: CliCommand::Balance {
                    pubkey: None,
                    use_lamports_unit: true
                },
                require_keypair: true
            }
        );

        // Test Cancel Subcommand
        let test_cancel =
            test_commands
                .clone()
                .get_matches_from(vec!["test", "cancel", &pubkey_string]);
        assert_eq!(
            parse_command(&test_cancel).unwrap(),
            CliCommandInfo {
                command: CliCommand::Cancel(pubkey),
                require_keypair: true
            }
        );

        // Test Confirm Subcommand
        let signature = Signature::new(&vec![1; 64]);
        let signature_string = format!("{:?}", signature);
        let test_confirm =
            test_commands
                .clone()
                .get_matches_from(vec!["test", "confirm", &signature_string]);
        assert_eq!(
            parse_command(&test_confirm).unwrap(),
            CliCommandInfo {
                command: CliCommand::Confirm(signature),
                require_keypair: false
            }
        );
        let test_bad_signature = test_commands
            .clone()
            .get_matches_from(vec!["test", "confirm", "deadbeef"]);
        assert!(parse_command(&test_bad_signature).is_err());

        // Test Deploy Subcommand
        let test_deploy =
            test_commands
                .clone()
                .get_matches_from(vec!["test", "deploy", "/Users/test/program.o"]);
        assert_eq!(
            parse_command(&test_deploy).unwrap(),
            CliCommandInfo {
                command: CliCommand::Deploy("/Users/test/program.o".to_string()),
                require_keypair: true
            }
        );

        // Test Simple Pay Subcommand
        let test_pay = test_commands.clone().get_matches_from(vec![
            "test",
            "pay",
            &pubkey_string,
            "50",
            "lamports",
        ]);
        assert_eq!(
            parse_command(&test_pay).unwrap(),
            CliCommandInfo {
                command: CliCommand::Pay {
                    lamports: 50,
                    to: pubkey,
                    timestamp: None,
                    timestamp_pubkey: None,
                    witnesses: None,
                    cancelable: false,
                },
                require_keypair: true
            }
        );

        // Test Pay Subcommand w/ Witness
        let test_pay_multiple_witnesses = test_commands.clone().get_matches_from(vec![
            "test",
            "pay",
            &pubkey_string,
            "50",
            "lamports",
            "--require-signature-from",
            &witness0_string,
            "--require-signature-from",
            &witness1_string,
        ]);
        assert_eq!(
            parse_command(&test_pay_multiple_witnesses).unwrap(),
            CliCommandInfo {
                command: CliCommand::Pay {
                    lamports: 50,
                    to: pubkey,
                    timestamp: None,
                    timestamp_pubkey: None,
                    witnesses: Some(vec![witness0, witness1]),
                    cancelable: false,
                },
                require_keypair: true
            }
        );
        let test_pay_single_witness = test_commands.clone().get_matches_from(vec![
            "test",
            "pay",
            &pubkey_string,
            "50",
            "lamports",
            "--require-signature-from",
            &witness0_string,
        ]);
        assert_eq!(
            parse_command(&test_pay_single_witness).unwrap(),
            CliCommandInfo {
                command: CliCommand::Pay {
                    lamports: 50,
                    to: pubkey,
                    timestamp: None,
                    timestamp_pubkey: None,
                    witnesses: Some(vec![witness0]),
                    cancelable: false,
                },
                require_keypair: true
            }
        );

        // Test Pay Subcommand w/ Timestamp
        let test_pay_timestamp = test_commands.clone().get_matches_from(vec![
            "test",
            "pay",
            &pubkey_string,
            "50",
            "lamports",
            "--after",
            "2018-09-19T17:30:59",
            "--require-timestamp-from",
            &witness0_string,
        ]);
        assert_eq!(
            parse_command(&test_pay_timestamp).unwrap(),
            CliCommandInfo {
                command: CliCommand::Pay {
                    lamports: 50,
                    to: pubkey,
                    timestamp: Some(dt),
                    timestamp_pubkey: Some(witness0),
                    witnesses: None,
                    cancelable: false,
                },
                require_keypair: true
            }
        );

        // Test Send-Signature Subcommand
        let test_send_signature = test_commands.clone().get_matches_from(vec![
            "test",
            "send-signature",
            &pubkey_string,
            &pubkey_string,
        ]);
        assert_eq!(
            parse_command(&test_send_signature).unwrap(),
            CliCommandInfo {
                command: CliCommand::Witness(pubkey, pubkey),
                require_keypair: true
            }
        );
        let test_pay_multiple_witnesses = test_commands.clone().get_matches_from(vec![
            "test",
            "pay",
            &pubkey_string,
            "50",
            "lamports",
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
            parse_command(&test_pay_multiple_witnesses).unwrap(),
            CliCommandInfo {
                command: CliCommand::Pay {
                    lamports: 50,
                    to: pubkey,
                    timestamp: Some(dt),
                    timestamp_pubkey: Some(witness0),
                    witnesses: Some(vec![witness0, witness1]),
                    cancelable: false,
                },
                require_keypair: true
            }
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
            parse_command(&test_send_timestamp).unwrap(),
            CliCommandInfo {
                command: CliCommand::TimeElapsed(pubkey, pubkey, dt),
                require_keypair: true
            }
        );
        let test_bad_timestamp = test_commands.clone().get_matches_from(vec![
            "test",
            "send-timestamp",
            &pubkey_string,
            &pubkey_string,
            "--date",
            "20180919T17:30:59",
        ]);
        assert!(parse_command(&test_bad_timestamp).is_err());
    }

    #[test]
    fn test_cli_process_command() {
        // Success cases
        let mut config = CliConfig::default();
        config.rpc_client = Some(RpcClient::new_mock("succeeds".to_string()));

        let keypair = Keypair::new();
        let pubkey = keypair.pubkey().to_string();
        config.keypair = keypair;
        config.command = CliCommand::Address;
        assert_eq!(process_command(&config).unwrap(), pubkey);

        config.command = CliCommand::Balance {
            pubkey: None,
            use_lamports_unit: true,
        };
        assert_eq!(process_command(&config).unwrap(), "50 lamports");

        config.command = CliCommand::Balance {
            pubkey: None,
            use_lamports_unit: false,
        };
        assert_eq!(process_command(&config).unwrap(), "0.00000005 SOL");

        let process_id = Pubkey::new_rand();
        config.command = CliCommand::Cancel(process_id);
        assert_eq!(process_command(&config).unwrap(), SIGNATURE);

        let good_signature = Signature::new(&bs58::decode(SIGNATURE).into_vec().unwrap());
        config.command = CliCommand::Confirm(good_signature);
        assert_eq!(process_command(&config).unwrap(), "Confirmed");

        let bob_pubkey = Pubkey::new_rand();
        let node_pubkey = Pubkey::new_rand();
        config.command = CliCommand::CreateVoteAccount {
            vote_account_pubkey: bob_pubkey,
            node_pubkey,
            authorized_voter: Some(bob_pubkey),
            authorized_withdrawer: Some(bob_pubkey),
            commission: 0,
        };
        let signature = process_command(&config);
        assert_eq!(signature.unwrap(), SIGNATURE.to_string());

        let new_authorized_pubkey = Pubkey::new_rand();
        config.command =
            CliCommand::VoteAuthorize(bob_pubkey, new_authorized_pubkey, VoteAuthorize::Voter);
        let signature = process_command(&config);
        assert_eq!(signature.unwrap(), SIGNATURE.to_string());

        let bob_pubkey = Pubkey::new_rand();
        let custodian = Pubkey::new_rand();
        config.command = CliCommand::CreateStakeAccount {
            stake_account_pubkey: bob_pubkey,
            staker: None,
            withdrawer: None,
            lockup: Lockup { slot: 0, custodian },
            lamports: 1234,
        };
        let signature = process_command(&config);
        assert_eq!(signature.unwrap(), SIGNATURE.to_string());

        let stake_pubkey = Pubkey::new_rand();
        let to_pubkey = Pubkey::new_rand();
        config.command = CliCommand::WithdrawStake(stake_pubkey, to_pubkey, 100);
        let signature = process_command(&config);
        assert_eq!(signature.unwrap(), SIGNATURE.to_string());

        let stake_pubkey = Pubkey::new_rand();
        config.command = CliCommand::DeactivateStake(stake_pubkey);
        let signature = process_command(&config);
        assert_eq!(signature.unwrap(), SIGNATURE.to_string());

        config.command = CliCommand::GetSlot;
        assert_eq!(process_command(&config).unwrap(), "0");

        config.command = CliCommand::GetTransactionCount;
        assert_eq!(process_command(&config).unwrap(), "1234");

        config.command = CliCommand::Pay {
            lamports: 10,
            to: bob_pubkey,
            timestamp: None,
            timestamp_pubkey: None,
            witnesses: None,
            cancelable: false,
        };
        let signature = process_command(&config);
        assert_eq!(signature.unwrap(), SIGNATURE.to_string());

        let date_string = "\"2018-09-19T17:30:59Z\"";
        let dt: DateTime<Utc> = serde_json::from_str(&date_string).unwrap();
        config.command = CliCommand::Pay {
            lamports: 10,
            to: bob_pubkey,
            timestamp: Some(dt),
            timestamp_pubkey: Some(config.keypair.pubkey()),
            witnesses: None,
            cancelable: false,
        };
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
        config.command = CliCommand::Pay {
            lamports: 10,
            to: bob_pubkey,
            timestamp: None,
            timestamp_pubkey: None,
            witnesses: Some(vec![witness]),
            cancelable: true,
        };
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
        config.command = CliCommand::TimeElapsed(bob_pubkey, process_id, dt);
        let signature = process_command(&config);
        assert_eq!(signature.unwrap(), SIGNATURE.to_string());

        let witness = Pubkey::new_rand();
        config.command = CliCommand::Witness(bob_pubkey, witness);
        let signature = process_command(&config);
        assert_eq!(signature.unwrap(), SIGNATURE.to_string());

        // Need airdrop cases
        config.command = CliCommand::Airdrop {
            drone_host: None,
            drone_port: 1234,
            lamports: 50,
            use_lamports_unit: true,
        };
        assert!(process_command(&config).is_ok());

        config.rpc_client = Some(RpcClient::new_mock("airdrop".to_string()));
        config.command = CliCommand::TimeElapsed(bob_pubkey, process_id, dt);
        let signature = process_command(&config);
        assert_eq!(signature.unwrap(), SIGNATURE.to_string());

        let witness = Pubkey::new_rand();
        config.command = CliCommand::Witness(bob_pubkey, witness);
        let signature = process_command(&config);
        assert_eq!(signature.unwrap(), SIGNATURE.to_string());

        // sig_not_found case
        config.rpc_client = Some(RpcClient::new_mock("sig_not_found".to_string()));
        let missing_signature = Signature::new(&bs58::decode("5VERv8NMvzbJMEkV8xnrLkEaWRtSz9CosKDYjCJjBRnbJLgp8uirBgmQpjKhoR4tjF3ZpRzrFmBV6UjKdiSZkQUW").into_vec().unwrap());
        config.command = CliCommand::Confirm(missing_signature);
        assert_eq!(process_command(&config).unwrap(), "Not found");

        // Tx error case
        config.rpc_client = Some(RpcClient::new_mock("account_in_use".to_string()));
        let any_signature = Signature::new(&bs58::decode(SIGNATURE).into_vec().unwrap());
        config.command = CliCommand::Confirm(any_signature);
        assert_eq!(
            process_command(&config).unwrap(),
            format!(
                "Transaction failed with error {:?}",
                TransactionError::AccountInUse
            )
        );

        // Failure cases
        config.rpc_client = Some(RpcClient::new_mock("fails".to_string()));

        config.command = CliCommand::Airdrop {
            drone_host: None,
            drone_port: 1234,
            lamports: 50,
            use_lamports_unit: true,
        };
        assert!(process_command(&config).is_err());

        config.command = CliCommand::Balance {
            pubkey: None,
            use_lamports_unit: false,
        };
        assert!(process_command(&config).is_err());

        config.command = CliCommand::CreateVoteAccount {
            vote_account_pubkey: bob_pubkey,
            node_pubkey,
            authorized_voter: Some(bob_pubkey),
            authorized_withdrawer: Some(bob_pubkey),
            commission: 0,
        };
        assert!(process_command(&config).is_err());

        config.command = CliCommand::VoteAuthorize(bob_pubkey, bob_pubkey, VoteAuthorize::Voter);
        assert!(process_command(&config).is_err());

        config.command = CliCommand::GetSlot;
        assert!(process_command(&config).is_err());

        config.command = CliCommand::GetTransactionCount;
        assert!(process_command(&config).is_err());

        config.command = CliCommand::Pay {
            lamports: 10,
            to: bob_pubkey,
            timestamp: None,
            timestamp_pubkey: None,
            witnesses: None,
            cancelable: false,
        };
        assert!(process_command(&config).is_err());

        config.command = CliCommand::Pay {
            lamports: 10,
            to: bob_pubkey,
            timestamp: Some(dt),
            timestamp_pubkey: Some(config.keypair.pubkey()),
            witnesses: None,
            cancelable: false,
        };
        assert!(process_command(&config).is_err());

        config.command = CliCommand::Pay {
            lamports: 10,
            to: bob_pubkey,
            timestamp: None,
            timestamp_pubkey: None,
            witnesses: Some(vec![witness]),
            cancelable: true,
        };
        assert!(process_command(&config).is_err());

        config.command = CliCommand::TimeElapsed(bob_pubkey, process_id, dt);
        assert!(process_command(&config).is_err());
    }

    #[test]
    fn test_cli_deploy() {
        solana_logger::setup();
        let mut pathbuf = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        pathbuf.push("tests");
        pathbuf.push("fixtures");
        pathbuf.push("noop");
        pathbuf.set_extension("so");

        // Success case
        let mut config = CliConfig::default();
        config.rpc_client = Some(RpcClient::new_mock("deploy_succeeds".to_string()));

        config.command = CliCommand::Deploy(pathbuf.to_str().unwrap().to_string());
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
        config.command = CliCommand::Deploy("bad/file/location.so".to_string());
        assert!(process_command(&config).is_err());
    }
}
