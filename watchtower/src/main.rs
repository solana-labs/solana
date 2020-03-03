//! A command-line executable for monitoring the health of a cluster

mod notifier;

use crate::notifier::Notifier;
use clap::{crate_description, crate_name, value_t_or_exit, App, Arg};
use log::*;
use solana_clap_utils::{
    input_parsers::pubkey_of,
    input_validators::{is_pubkey_or_keypair, is_url},
};
use solana_client::rpc_client::RpcClient;
use solana_metrics::{datapoint_error, datapoint_info};
use std::{error, io, thread::sleep, time::Duration};

fn main() -> Result<(), Box<dyn error::Error>> {
    let matches = App::new(crate_name!())
        .about(crate_description!())
        .version(solana_clap_utils::version!())
        .arg(
            Arg::with_name("json_rpc_url")
                .long("url")
                .value_name("URL")
                .takes_value(true)
                .required(true)
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
            Arg::with_name("validator_identity")
                .long("validator-identity")
                .value_name("VALIDATOR IDENTITY PUBKEY")
                .takes_value(true)
                .validator(is_pubkey_or_keypair)
                .help("Monitor a specific validator only instead of the entire cluster"),
        )
        .arg(
            Arg::with_name("no_duplicate_notifications")
                .long("no-duplicate-notifications")
                .takes_value(false)
                .help("Subsequent identical notifications will be suppressed"),
        )
        .get_matches();

    let interval = Duration::from_secs(value_t_or_exit!(matches, "interval", u64));
    let json_rpc_url = value_t_or_exit!(matches, "json_rpc_url", String);
    let validator_identity = pubkey_of(&matches, "validator_identity").map(|i| i.to_string());
    let no_duplicate_notifications = matches.is_present("no_duplicate_notifications");

    solana_logger::setup_with_default("solana=info");
    solana_metrics::set_panic_hook("watchtower");

    let rpc_client = RpcClient::new(json_rpc_url);

    let notifier = Notifier::new();
    let mut last_transaction_count = 0;
    let mut last_check_notification_sent = false;
    let mut last_notification_msg = String::from("");
    loop {
        let mut notify_msg = String::from("solana-watchtower: undefined error");
        let ok = rpc_client
            .get_transaction_count()
            .and_then(|transaction_count| {
                info!("Current transaction count: {}", transaction_count);

                if transaction_count > last_transaction_count {
                    last_transaction_count = transaction_count;
                    Ok(true)
                } else {
                    Err(io::Error::new(
                        io::ErrorKind::Other,
                        format!(
                            "Transaction count is not advancing: {} <= {}",
                            transaction_count, last_transaction_count
                        ),
                    ))
                }
            })
            .unwrap_or_else(|err| {
                notify_msg = format!("solana-watchtower: {}", err.to_string());
                datapoint_error!(
                    "watchtower-sanity-failure",
                    ("test", "transaction-count", String),
                    ("err", err.to_string(), String)
                );
                false
            })
            && rpc_client
                .get_recent_blockhash()
                .and_then(|(blockhash, _fee_calculator)| {
                    info!("Current blockhash: {}", blockhash);
                    rpc_client.get_new_blockhash(&blockhash)
                })
                .and_then(|(blockhash, _fee_calculator)| {
                    info!("New blockhash: {}", blockhash);
                    Ok(true)
                })
                .unwrap_or_else(|err| {
                    notify_msg = format!("solana-watchtower: {}", err.to_string());
                    datapoint_error!(
                        "watchtower-sanity-failure",
                        ("test", "blockhash", String),
                        ("err", err.to_string(), String)
                    );
                    false
                })
            && rpc_client
                .get_vote_accounts()
                .and_then(|vote_accounts| {
                    info!("Current validator count: {}", vote_accounts.current.len());
                    info!(
                        "Delinquent validator count: {}",
                        vote_accounts.delinquent.len()
                    );

                    match validator_identity.as_ref() {
                        Some(validator_identity) => {
                            if vote_accounts
                                .current
                                .iter()
                                .any(|vai| vai.node_pubkey == *validator_identity)
                            {
                                Ok(true)
                            } else if vote_accounts
                                .delinquent
                                .iter()
                                .any(|vai| vai.node_pubkey == *validator_identity)
                            {
                                Err(io::Error::new(
                                    io::ErrorKind::Other,
                                    format!("Validator {} is delinquent", validator_identity),
                                ))
                            } else {
                                Err(io::Error::new(
                                    io::ErrorKind::Other,
                                    format!("Validator {} is missing", validator_identity),
                                ))
                            }
                        }
                        None => {
                            if vote_accounts.delinquent.is_empty() {
                                Ok(true)
                            } else {
                                Err(io::Error::new(
                                    io::ErrorKind::Other,
                                    format!(
                                        "{} delinquent validators",
                                        vote_accounts.delinquent.len()
                                    ),
                                ))
                            }
                        }
                    }
                })
                .unwrap_or_else(|err| {
                    notify_msg = format!("solana-watchtower: {}", err.to_string());
                    datapoint_error!(
                        "watchtower-sanity-failure",
                        ("test", "delinquent-validators", String),
                        ("err", err.to_string(), String)
                    );
                    false
                });

        datapoint_info!("watchtower-sanity", ("ok", ok, bool));
        if !ok {
            last_check_notification_sent = true;
            if no_duplicate_notifications {
                if last_notification_msg != notify_msg {
                    notifier.send(&notify_msg);
                    last_notification_msg = notify_msg;
                } else {
                    datapoint_info!(
                        "watchtower-sanity",
                        ("Suppressing duplicate notification", ok, bool)
                    );
                }
            } else {
                notifier.send(&notify_msg);
            }
        } else {
            if last_check_notification_sent {
                notifier.send("solana-watchtower: All Clear");
            }
            last_check_notification_sent = false;
            last_notification_msg = String::from("");
        }
        sleep(interval);
    }
}
