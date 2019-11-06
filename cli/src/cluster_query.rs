use crate::{
    cli::{
        build_balance_message, check_account_for_fee, CliCommand, CliCommandInfo, CliConfig,
        CliError, ProcessResult,
    },
    display::println_name_value,
};
use clap::{value_t_or_exit, App, Arg, ArgMatches, SubCommand};
use console::{style, Emoji};
use serde_json::Value;
use solana_client::{rpc_client::RpcClient, rpc_request::RpcVoteAccountInfo};
use solana_sdk::{
    clock,
    commitment_config::CommitmentConfig,
    hash::Hash,
    signature::{Keypair, KeypairUtil},
    system_transaction,
};
use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};

static CHECK_MARK: Emoji = Emoji("✅ ", "");
static CROSS_MARK: Emoji = Emoji("❌ ", "");
static WARNING: Emoji = Emoji("⚠️", "!");

pub trait ClusterQuerySubCommands {
    fn cluster_query_subcommands(self) -> Self;
}

impl ClusterQuerySubCommands for App<'_, '_> {
    fn cluster_query_subcommands(self) -> Self {
        self.subcommand(
            SubCommand::with_name("cluster-version")
                .about("Get the version of the cluster entrypoint"),
        )
        .subcommand(SubCommand::with_name("fees").about("Display current cluster fees"))
        .subcommand(
            SubCommand::with_name("get-epoch-info")
                .about("Get information about the current epoch"),
        )
        .subcommand(
            SubCommand::with_name("get-genesis-blockhash").about("Get the genesis blockhash"),
        )
        .subcommand(SubCommand::with_name("get-slot").about("Get current slot"))
        .subcommand(
            SubCommand::with_name("get-transaction-count").about("Get current transaction count"),
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
                        .default_value("15")
                        .help("Wait up to timeout seconds for transaction confirmation"),
                ),
        )
        .subcommand(
            SubCommand::with_name("show-validators")
                .about("Show information about the current validators")
                .arg(
                    Arg::with_name("lamports")
                        .long("lamports")
                        .takes_value(false)
                        .help("Display balance in lamports instead of SOL"),
                ),
        )
    }
}

pub fn parse_cluster_ping(matches: &ArgMatches<'_>) -> Result<CliCommandInfo, CliError> {
    let interval = Duration::from_secs(value_t_or_exit!(matches, "interval", u64));
    let count = if matches.is_present("count") {
        Some(value_t_or_exit!(matches, "count", u64))
    } else {
        None
    };
    let timeout = Duration::from_secs(value_t_or_exit!(matches, "timeout", u64));
    Ok(CliCommandInfo {
        command: CliCommand::Ping {
            interval,
            count,
            timeout,
        },
        require_keypair: true,
    })
}

pub fn parse_show_validators(matches: &ArgMatches<'_>) -> Result<CliCommandInfo, CliError> {
    let use_lamports_unit = matches.is_present("lamports");

    Ok(CliCommandInfo {
        command: CliCommand::ShowValidators { use_lamports_unit },
        require_keypair: false,
    })
}

pub fn process_cluster_version(rpc_client: &RpcClient, config: &CliConfig) -> ProcessResult {
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

pub fn process_fees(rpc_client: &RpcClient) -> ProcessResult {
    let (recent_blockhash, fee_calculator) = rpc_client.get_recent_blockhash()?;

    Ok(format!(
        "blockhash: {}\nlamports per signature: {}",
        recent_blockhash, fee_calculator.lamports_per_signature
    ))
}

pub fn process_get_epoch_info(rpc_client: &RpcClient) -> ProcessResult {
    let epoch_info = rpc_client.get_epoch_info()?;
    println!();
    println_name_value("Current epoch:", &epoch_info.epoch.to_string());
    println_name_value("Current slot:", &epoch_info.absolute_slot.to_string());
    println_name_value(
        "Total slots in current epoch:",
        &epoch_info.slots_in_epoch.to_string(),
    );
    let remaining_slots_in_epoch = epoch_info.slots_in_epoch - epoch_info.slot_index;
    println_name_value(
        "Remaining slots in current epoch:",
        &remaining_slots_in_epoch.to_string(),
    );

    let remaining_time_in_epoch = Duration::from_secs(
        remaining_slots_in_epoch * clock::DEFAULT_TICKS_PER_SLOT / clock::DEFAULT_TICKS_PER_SECOND,
    );
    println_name_value(
        "Time remaining in current epoch:",
        &format!(
            "{} minutes, {} seconds",
            remaining_time_in_epoch.as_secs() / 60,
            remaining_time_in_epoch.as_secs() % 60
        ),
    );
    Ok("".to_string())
}

pub fn process_get_genesis_blockhash(rpc_client: &RpcClient) -> ProcessResult {
    let genesis_blockhash = rpc_client.get_genesis_blockhash()?;
    Ok(genesis_blockhash.to_string())
}

pub fn process_get_slot(rpc_client: &RpcClient) -> ProcessResult {
    let slot = rpc_client.get_slot()?;
    Ok(slot.to_string())
}

pub fn process_get_transaction_count(rpc_client: &RpcClient) -> ProcessResult {
    let transaction_count = rpc_client.get_transaction_count()?;
    Ok(transaction_count.to_string())
}

pub fn process_ping(
    rpc_client: &RpcClient,
    config: &CliConfig,
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
                    let signature_status = rpc_client.get_signature_status_with_commitment(
                        &signature,
                        CommitmentConfig::recent(),
                    )?;
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
                            500 * solana_sdk::clock::DEFAULT_TICKS_PER_SLOT
                                / solana_sdk::clock::DEFAULT_TICKS_PER_SECOND,
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

pub fn process_show_validators(rpc_client: &RpcClient, use_lamports_unit: bool) -> ProcessResult {
    let vote_accounts = rpc_client.get_vote_accounts()?;
    let total_active_stake = vote_accounts
        .current
        .iter()
        .chain(vote_accounts.delinquent.iter())
        .fold(0, |acc, vote_account| acc + vote_account.activated_stake)
        as f64;

    let total_deliquent_stake = vote_accounts
        .delinquent
        .iter()
        .fold(0, |acc, vote_account| acc + vote_account.activated_stake)
        as f64;
    let total_current_stake = total_active_stake - total_deliquent_stake;

    println_name_value(
        "Active Stake:",
        &build_balance_message(total_active_stake as u64, use_lamports_unit, true),
    );
    if total_deliquent_stake > 0. {
        println_name_value(
            "Current Stake:",
            &format!(
                "{} ({:0.2}%)",
                &build_balance_message(total_current_stake as u64, use_lamports_unit, true),
                100. * total_current_stake / total_active_stake
            ),
        );
        println_name_value(
            "Delinquent Stake:",
            &format!(
                "{} ({:0.2}%)",
                &build_balance_message(total_deliquent_stake as u64, use_lamports_unit, true),
                100. * total_deliquent_stake / total_active_stake
            ),
        );
    }
    println!();

    println!(
        "{}",
        style(format!(
            "  {:<44}  {:<44}  {:<11}  {:>10}  {:>11}  {}",
            "Identity Pubkey",
            "Vote Account Pubkey",
            "Commission",
            "Last Vote",
            "Root Block",
            "Active Stake",
        ))
        .bold()
    );

    fn print_vote_account(
        vote_account: &RpcVoteAccountInfo,
        total_active_stake: f64,
        use_lamports_unit: bool,
        delinquent: bool,
    ) {
        fn non_zero_or_dash(v: u64) -> String {
            if v == 0 {
                "-".into()
            } else {
                format!("{}", v)
            }
        }
        println!(
            "{} {:<44}  {:<44}  {:>3} ({:>4.1}%)  {:>10}  {:>11}  {:>11}",
            if delinquent {
                WARNING.to_string()
            } else {
                " ".to_string()
            },
            vote_account.node_pubkey,
            vote_account.vote_pubkey,
            vote_account.commission,
            f64::from(vote_account.commission) * 100.0 / f64::from(std::u8::MAX),
            non_zero_or_dash(vote_account.last_vote),
            non_zero_or_dash(vote_account.root_slot),
            if vote_account.activated_stake > 0 {
                format!(
                    "{} ({:.2}%)",
                    build_balance_message(vote_account.activated_stake, use_lamports_unit, true),
                    100. * vote_account.activated_stake as f64 / total_active_stake
                )
            } else {
                "-".into()
            },
        );
    }

    for vote_account in vote_accounts.current.iter() {
        print_vote_account(vote_account, total_active_stake, use_lamports_unit, false);
    }
    for vote_account in vote_accounts.delinquent.iter() {
        print_vote_account(vote_account, total_active_stake, use_lamports_unit, true);
    }

    Ok("".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::{app, parse_command};

    #[test]
    fn test_parse_command() {
        let test_commands = app("test", "desc", "version");

        let test_cluster_version = test_commands
            .clone()
            .get_matches_from(vec!["test", "cluster-version"]);
        assert_eq!(
            parse_command(&test_cluster_version).unwrap(),
            CliCommandInfo {
                command: CliCommand::ClusterVersion,
                require_keypair: false
            }
        );

        let test_fees = test_commands.clone().get_matches_from(vec!["test", "fees"]);
        assert_eq!(
            parse_command(&test_fees).unwrap(),
            CliCommandInfo {
                command: CliCommand::Fees,
                require_keypair: false
            }
        );

        let test_get_epoch_info = test_commands
            .clone()
            .get_matches_from(vec!["test", "get-epoch-info"]);
        assert_eq!(
            parse_command(&test_get_epoch_info).unwrap(),
            CliCommandInfo {
                command: CliCommand::GetEpochInfo,
                require_keypair: false
            }
        );

        let test_get_genesis_blockhash = test_commands
            .clone()
            .get_matches_from(vec!["test", "get-genesis-blockhash"]);
        assert_eq!(
            parse_command(&test_get_genesis_blockhash).unwrap(),
            CliCommandInfo {
                command: CliCommand::GetGenesisBlockhash,
                require_keypair: false
            }
        );

        let test_get_slot = test_commands
            .clone()
            .get_matches_from(vec!["test", "get-slot"]);
        assert_eq!(
            parse_command(&test_get_slot).unwrap(),
            CliCommandInfo {
                command: CliCommand::GetSlot,
                require_keypair: false
            }
        );

        let test_transaction_count = test_commands
            .clone()
            .get_matches_from(vec!["test", "get-transaction-count"]);
        assert_eq!(
            parse_command(&test_transaction_count).unwrap(),
            CliCommandInfo {
                command: CliCommand::GetTransactionCount,
                require_keypair: false
            }
        );

        let test_ping = test_commands
            .clone()
            .get_matches_from(vec!["test", "ping", "-i", "1", "-c", "2", "-t", "3"]);
        assert_eq!(
            parse_command(&test_ping).unwrap(),
            CliCommandInfo {
                command: CliCommand::Ping {
                    interval: Duration::from_secs(1),
                    count: Some(2),
                    timeout: Duration::from_secs(3),
                },
                require_keypair: true
            }
        );
    }
    // TODO: Add process tests
}
