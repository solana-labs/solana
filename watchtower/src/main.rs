//! A command-line executable for monitoring the health of a cluster

mod notifier;

use crate::notifier::Notifier;
use clap::{crate_description, crate_name, value_t_or_exit, App, Arg};
use log::*;
use solana_clap_utils::input_validators::is_url;
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
        .get_matches();

    let interval = Duration::from_secs(value_t_or_exit!(matches, "interval", u64));
    let json_rpc_url = value_t_or_exit!(matches, "json_rpc_url", String);

    solana_logger::setup_with_filter("solana=info");
    solana_metrics::set_panic_hook("watchtower");

    let rpc_client = RpcClient::new(json_rpc_url.to_string());

    let notifier = Notifier::new();
    let mut last_transaction_count = 0;
    loop {
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
                    if vote_accounts.delinquent.is_empty() {
                        Ok(true)
                    } else {
                        Err(io::Error::new(
                            io::ErrorKind::Other,
                            format!("{} delinquent validators", vote_accounts.delinquent.len()),
                        ))
                    }
                })
                .unwrap_or_else(|err| {
                    datapoint_error!(
                        "watchtower-sanity-failure",
                        ("test", "delinquent-validators", String),
                        ("err", err.to_string(), String)
                    );
                    false
                });

        datapoint_info!("watchtower-sanity", ("ok", ok, bool));
        if !ok {
            notifier.send("solana-watchtower sanity failure");
        }
        sleep(interval);
    }
}
