//! A command-line executable for monitoring the health of a cluster

use clap::{crate_description, crate_name, value_t, value_t_or_exit, App, Arg};
use log::*;
use solana_clap_utils::{
    input_parsers::pubkeys_of,
    input_validators::{is_pubkey_or_keypair, is_url},
};
use solana_client::{
    client_error::Result as ClientResult, rpc_client::RpcClient, rpc_response::RpcVoteAccountStatus,
};
use solana_metrics::{datapoint_error, datapoint_info};
use solana_notifier::Notifier;
use solana_sdk::{
    clock::Slot, hash::Hash, native_token::lamports_to_sol, program_utils::limited_deserialize,
    pubkey::Pubkey,
};
use solana_transaction_status::{ConfirmedBlock, TransactionEncoding};
use solana_vote_program::vote_instruction::VoteInstruction;
use std::{
    error,
    str::FromStr,
    thread::sleep,
    time::{Duration, Instant},
};

struct Config {
    interval: Duration,
    json_rpc_url: String,
    validator_identity_pubkeys: Vec<String>,
    no_duplicate_notifications: bool,
    monitor_active_stake: bool,
    notify_on_transactions: bool,
}

fn get_config() -> Config {
    let matches = App::new(crate_name!())
        .about(crate_description!())
        .version(solana_version::version!())
        .after_help("ADDITIONAL HELP:
        To receive a Slack, Discord and/or Telegram notification on sanity failure,
        define environment variables before running `solana-watchtower`:

        export SLACK_WEBHOOK=...
        export DISCORD_WEBHOOK=...

        Telegram requires the following two variables:

        export TELEGRAM_BOT_TOKEN=...
        export TELEGRAM_CHAT_ID=...

        To receive a Twilio SMS notification on failure, having a Twilio account,
        and a sending number owned by that account,
        define environment variable before running `solana-watchtower`:

        export TWILIO_CONFIG='ACCOUNT=<account>,TOKEN=<securityToken>,TO=<receivingNumber>,FROM=<sendingNumber>'")
        .arg({
            let arg = Arg::with_name("config_file")
                .short("C")
                .long("config")
                .value_name("PATH")
                .takes_value(true)
                .global(true)
                .help("Configuration file to use");
            if let Some(ref config_file) = *solana_cli_config::CONFIG_FILE {
                arg.default_value(&config_file)
            } else {
                arg
            }
        })
        .arg(
            Arg::with_name("json_rpc_url")
                .long("url")
                .value_name("URL")
                .takes_value(true)
                .validator(is_url)
                .help("JSON RPC URL for the cluster"),
        )
        .arg(
            Arg::with_name("interval")
                .long("interval")
                .value_name("SECONDS")
                .takes_value(true)
                .default_value("60")
                .help("Wait interval seconds between checking the cluster"),
        )
        .arg(
            Arg::with_name("validator_identities")
                .long("validator-identity")
                .value_name("VALIDATOR IDENTITY PUBKEY")
                .takes_value(true)
                .validator(is_pubkey_or_keypair)
                .multiple(true)
                .help("Monitor a specific validator only instead of the entire cluster"),
        )
        .arg(
            Arg::with_name("no_duplicate_notifications")
                .long("no-duplicate-notifications")
                .takes_value(false)
                .help("Subsequent identical notifications will be suppressed"),
        )
        .arg(
            Arg::with_name("monitor_active_stake")
                .long("monitor-active-stake")
                .takes_value(false)
                .help("Alert when the current stake for the cluster drops below 80%"),
        )
        .arg(
            Arg::with_name("notify_on_transactions")
                .long("notify-on-transactions")
                .takes_value(false)
                .help("Send a notification on all non-vote transactions.  This can be very verbose!\
                    Note that the notification environment variables used by this feature all require a \
                    TRANSACTION_NOTIFIER_ prefix.  For example: TRANSACTION_NOTIFIER_SLACK_WEBHOOK"),
        )
        .get_matches();

    let config = if let Some(config_file) = matches.value_of("config_file") {
        solana_cli_config::Config::load(config_file).unwrap_or_default()
    } else {
        solana_cli_config::Config::default()
    };

    let interval = Duration::from_secs(value_t_or_exit!(matches, "interval", u64));
    let json_rpc_url =
        value_t!(matches, "json_rpc_url", String).unwrap_or_else(|_| config.json_rpc_url);
    let validator_identity_pubkeys: Vec<_> = pubkeys_of(&matches, "validator_identities")
        .unwrap_or_else(|| vec![])
        .into_iter()
        .map(|i| i.to_string())
        .collect();

    let no_duplicate_notifications = matches.is_present("no_duplicate_notifications");
    let monitor_active_stake = matches.is_present("monitor_active_stake");
    let notify_on_transactions = matches.is_present("notify_on_transactions");

    let config = Config {
        interval,
        json_rpc_url,
        validator_identity_pubkeys,
        no_duplicate_notifications,
        monitor_active_stake,
        notify_on_transactions,
    };

    info!("RPC URL: {}", config.json_rpc_url);
    if !config.validator_identity_pubkeys.is_empty() {
        info!(
            "Monitored validators: {:?}",
            config.validator_identity_pubkeys
        );
    }
    config
}

fn process_confirmed_block(notifier: &Notifier, slot: Slot, confirmed_block: ConfirmedBlock) {
    let mut vote_transactions = 0;

    for rpc_transaction in &confirmed_block.transactions {
        if let Some(transaction) = rpc_transaction.transaction.decode() {
            if transaction.verify().is_ok() {
                let mut notify = true;

                // Ignore simple Vote transactions since they are too prevalent
                if transaction.message.instructions.len() == 1 {
                    let instruction = &transaction.message.instructions[0];
                    let program_pubkey =
                        transaction.message.account_keys[instruction.program_id_index as usize];
                    if program_pubkey == solana_vote_program::id() {
                        if let Ok(VoteInstruction::Vote(_)) =
                            limited_deserialize::<VoteInstruction>(&instruction.data)
                        {
                            vote_transactions += 1;
                            notify = false;
                        }
                    }
                }

                if notify {
                    let mut w = Vec::new();
                    if solana_cli::display::write_transaction(
                        &mut w,
                        &transaction,
                        &rpc_transaction.meta,
                        "",
                    )
                    .is_ok()
                    {
                        if let Ok(s) = String::from_utf8(w) {
                            notifier.send(&format!("```Slot: {}\n{}```", slot, s));
                        }
                    }
                }
            } else {
                datapoint_error!(
                    "watchtower-sanity-failure",
                    ("slot", slot, i64),
                    ("err", "Transaction signature verification failed", String)
                );
            }
        }
    }
    info!(
        "Process slot {} with {} regular transactions (and {} votes)",
        slot,
        confirmed_block.transactions.len() - vote_transactions,
        vote_transactions
    );
}

fn load_blocks(
    rpc_client: &RpcClient,
    start_slot: Slot,
    end_slot: Slot,
) -> ClientResult<Vec<(Slot, ConfirmedBlock)>> {
    info!(
        "Loading confirmed blocks between slots: {} - {}",
        start_slot, end_slot
    );

    let slots = rpc_client.get_confirmed_blocks(start_slot, Some(end_slot))?;

    let mut blocks = vec![];
    for slot in slots.into_iter() {
        let block =
            rpc_client.get_confirmed_block_with_encoding(slot, TransactionEncoding::Binary)?;
        blocks.push((slot, block));
    }
    Ok(blocks)
}

fn transaction_monitor(rpc_client: RpcClient) {
    let notifier = Notifier::new("TRANSACTION_NOTIFIER_");
    let mut start_slot = loop {
        match rpc_client.get_slot() {
            Ok(slot) => break slot,
            Err(err) => {
                warn!("Failed to get current slot: {}", err);
            }
        }
        sleep(Duration::from_secs(1));
    };

    loop {
        let end_slot = start_slot + 50;
        info!("start_slot:{} - end_slot:{}", start_slot, end_slot);

        let latest_available_slot = rpc_client.get_slot().unwrap_or_else(|err| {
            info!("get_slot() failed: {}", err);
            0
        });

        if latest_available_slot <= start_slot {
            info!("Waiting for a slot greater than {}...", start_slot);
            sleep(Duration::from_secs(5));
            continue;
        }

        match load_blocks(&rpc_client, start_slot + 1, end_slot) {
            Ok(blocks) => {
                info!("Loaded {} blocks", blocks.len());

                if blocks.is_empty() && end_slot < latest_available_slot {
                    start_slot = end_slot;
                } else {
                    for (slot, block) in blocks.into_iter() {
                        process_confirmed_block(&notifier, slot, block);
                        start_slot = slot;
                    }
                }
            }
            Err(err) => {
                info!(
                    "failed to get blocks in range ({},{}): {}",
                    start_slot, end_slot, err
                );
                sleep(Duration::from_secs(1));
            }
        }
    }
}

fn get_cluster_info(rpc_client: &RpcClient) -> ClientResult<(u64, Hash, RpcVoteAccountStatus)> {
    let transaction_count = rpc_client.get_transaction_count()?;
    let recent_blockhash = rpc_client.get_recent_blockhash()?.0;
    let vote_accounts = rpc_client.get_vote_accounts()?;
    Ok((transaction_count, recent_blockhash, vote_accounts))
}

fn main() -> Result<(), Box<dyn error::Error>> {
    let config = get_config();

    solana_logger::setup_with_default("solana=info");
    solana_metrics::set_panic_hook("watchtower");

    let _notify_thread = if config.notify_on_transactions {
        let rpc_client = RpcClient::new(config.json_rpc_url.clone());
        Some(std::thread::spawn(move || transaction_monitor(rpc_client)))
    } else {
        None
    };

    let rpc_client = RpcClient::new(config.json_rpc_url.clone());
    let notifier = Notifier::default();
    let mut last_transaction_count = 0;
    let mut last_recent_blockhash = Hash::default();
    let mut last_notification_msg = "".into();
    let mut last_success = Instant::now();

    loop {
        let failure = match get_cluster_info(&rpc_client) {
            Ok((transaction_count, recent_blockhash, vote_accounts)) => {
                info!("Current transaction count: {}", transaction_count);
                info!("Recent blockhash: {}", recent_blockhash);
                info!("Current validator count: {}", vote_accounts.current.len());
                info!(
                    "Delinquent validator count: {}",
                    vote_accounts.delinquent.len()
                );

                let mut failures = vec![];

                let total_current_stake = vote_accounts
                    .current
                    .iter()
                    .fold(0, |acc, vote_account| acc + vote_account.activated_stake);
                let total_delinquent_stake = vote_accounts
                    .delinquent
                    .iter()
                    .fold(0, |acc, vote_account| acc + vote_account.activated_stake);

                let total_stake = total_current_stake + total_delinquent_stake;
                let current_stake_percent = total_current_stake * 100 / total_stake;
                info!(
                    "Current stake: {}% | Total stake: {} SOL, current stake: {} SOL, delinquent: {} SOL",
                    current_stake_percent,
                    lamports_to_sol(total_stake),
                    lamports_to_sol(total_current_stake),
                    lamports_to_sol(total_delinquent_stake)
                );

                if transaction_count > last_transaction_count {
                    last_transaction_count = transaction_count;
                } else {
                    failures.push((
                        "transaction-count",
                        format!(
                            "Transaction count is not advancing: {} <= {}",
                            transaction_count, last_transaction_count
                        ),
                    ));
                }

                if recent_blockhash != last_recent_blockhash {
                    last_recent_blockhash = recent_blockhash;
                } else {
                    failures.push((
                        "recent-blockhash",
                        format!("Unable to get new blockhash: {}", recent_blockhash),
                    ));
                }

                if config.monitor_active_stake && current_stake_percent < 80 {
                    failures.push((
                        "current-stake",
                        format!("Current stake is {}%", current_stake_percent),
                    ));
                }

                if config.validator_identity_pubkeys.is_empty() {
                    if !vote_accounts.delinquent.is_empty() {
                        failures.push((
                            "delinquent",
                            format!("{} delinquent validators", vote_accounts.delinquent.len()),
                        ));
                    }
                } else {
                    let mut errors = vec![];
                    for validator_identity in config.validator_identity_pubkeys.iter() {
                        if vote_accounts
                            .delinquent
                            .iter()
                            .any(|vai| vai.node_pubkey == *validator_identity)
                        {
                            errors.push(format!("{} delinquent", validator_identity));
                        } else if !vote_accounts
                            .current
                            .iter()
                            .any(|vai| vai.node_pubkey == *validator_identity)
                        {
                            errors.push(format!("{} missing", validator_identity));
                        }

                        rpc_client
                            .get_balance(&Pubkey::from_str(&validator_identity).unwrap_or_default())
                            .map(lamports_to_sol)
                            .map(|balance| {
                                if balance < 10.0 {
                                    // At 1 SOL/day for validator voting fees, this gives over a week to
                                    // find some more SOL
                                    failures.push((
                                        "balance",
                                        format!("{} has {} SOL", validator_identity, balance),
                                    ));
                                }
                            })
                            .unwrap_or_else(|err| {
                                warn!("Failed to get balance of {}: {:?}", validator_identity, err);
                            });
                    }

                    if !errors.is_empty() {
                        failures.push(("delinquent", errors.join(",")));
                    }
                }

                for failure in failures.iter() {
                    error!("{} sanity failure: {}", failure.0, failure.1);
                }
                failures.into_iter().next() // Only report the first failure if any
            }
            Err(err) => Some(("rpc", err.to_string())),
        };

        datapoint_info!("watchtower-sanity", ("ok", failure.is_none(), bool));
        if let Some((failure_test_name, failure_error_message)) = &failure {
            let notification_msg = format!(
                "solana-watchtower: Error: {}: {}",
                failure_test_name, failure_error_message
            );
            if !config.no_duplicate_notifications || last_notification_msg != notification_msg {
                notifier.send(&notification_msg);
            }
            datapoint_error!(
                "watchtower-sanity-failure",
                ("test", failure_test_name, String),
                ("err", failure_error_message, String)
            );
            last_notification_msg = notification_msg;
        } else {
            if !last_notification_msg.is_empty() {
                let alarm_duration = Instant::now().duration_since(last_success);
                let alarm_duration = Duration::from_secs(alarm_duration.as_secs()); // Drop milliseconds in message

                let all_clear_msg = format!(
                    "All clear after {}",
                    humantime::format_duration(alarm_duration)
                );
                info!("{}", all_clear_msg);
                notifier.send(&format!("solana-watchtower: {}", all_clear_msg));
            }
            last_notification_msg = "".into();
            last_success = Instant::now();
        }
        sleep(config.interval);
    }
}
