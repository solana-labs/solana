use clap::{
    crate_description, crate_name, value_t, value_t_or_exit, App, AppSettings, Arg, SubCommand,
};
use console::Emoji;
use log::*;
use solana_clap_utils::{
    input_parsers::pubkey_of,
    input_validators::{is_pubkey, is_slot, is_url},
};
use solana_client::rpc_client::RpcClient;
use solana_metrics::datapoint_error;
use solana_sdk::{clock::Slot, native_token::lamports_to_sol, pubkey::Pubkey};
use solana_stake_monitor::*;
use std::{fs, io, process};

fn load_stake_accounts_info(data_file: &str) -> StakeAccountsInfo {
    let data_file_new = data_file.to_owned() + "new";
    let stake_accounts_info = solana_cli_config::load_config_file(&data_file_new)
        .or_else(|_| solana_cli_config::load_config_file(data_file))
        .unwrap_or_default();

    // Ensure `data_file` always exists
    save_stake_accounts_info(data_file, &stake_accounts_info).expect("save_stake_accounts_info");

    stake_accounts_info
}

fn save_stake_accounts_info(
    data_file: &str,
    stake_accounts_info: &StakeAccountsInfo,
) -> io::Result<()> {
    let data_file_new = data_file.to_owned() + "new";
    solana_cli_config::save_config_file(&stake_accounts_info, &data_file_new)?;
    let _ = fs::remove_file(data_file);
    fs::rename(&data_file_new, data_file)
}

fn command_record(data_file: String, json_rpc_url: String, first_slot: Slot, batch_size: u64) {
    let mut stake_accounts_info = load_stake_accounts_info(&data_file);

    info!("RPC URL: {}", json_rpc_url);
    let rpc_client = RpcClient::new(json_rpc_url);
    if stake_accounts_info.slot < first_slot {
        stake_accounts_info.slot = first_slot;
    }
    loop {
        process_slots(&rpc_client, &mut stake_accounts_info, batch_size);
        save_stake_accounts_info(&data_file, &stake_accounts_info).unwrap_or_else(|err| {
            datapoint_error!(
                "stake-monitor-failure",
                (
                    "err",
                    format!("failed to save stake_accounts_info: {}", err),
                    String
                )
            );
        });
    }
}

fn command_check(data_file: String, stake_account_pubkey: Pubkey) {
    let stake_accounts_info = load_stake_accounts_info(&data_file);

    if let Some(stake_account_info) = stake_accounts_info
        .account_info
        .get(&stake_account_pubkey.to_string())
    {
        if let Some(slot) = stake_account_info.compliant_since {
            println!(
                "{}Stake account compliant since slot {} with a balance of {} SOL",
                Emoji("✅ ", ""),
                slot,
                lamports_to_sol(stake_account_info.lamports)
            );
            process::exit(0);
        } else {
            eprintln!(
                "{}Stake account not compliant due to: {:?}",
                Emoji("❌ ", ""),
                stake_account_info.transactions.last().unwrap()
            );
            process::exit(1);
        }
    } else {
        eprintln!("{} Unknown stake account", Emoji("⚠️ ", ""));
        process::exit(1);
    }
}

fn main() {
    solana_logger::setup_with_default("solana=info");
    solana_metrics::set_panic_hook("stake-monitor");

    let matches = App::new(crate_name!())
        .about(crate_description!())
        .version(solana_clap_utils::version!())
        .setting(AppSettings::SubcommandRequiredElseHelp)
        .arg(
            Arg::with_name("data_file")
                .long("data-file")
                .value_name("PATH")
                .takes_value(true)
                .default_value("stake-info.yml")
                .global(true)
                .help(
                    "Output YAML file that receives the information for all stake accounts.\
                       This file is updated atomically after each batch of slots is processed.",
                ),
        )
        .subcommand(
            SubCommand::with_name("record")
                .about("Monitor all Cluster transactions for state account compliance")
                .arg({
                    let arg = Arg::with_name("config_file")
                        .short("C")
                        .long("config")
                        .value_name("PATH")
                        .takes_value(true)
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
                    Arg::with_name("first_slot")
                        .long("--first-slot")
                        .value_name("SLOT")
                        .validator(is_slot)
                        .takes_value(true)
                        .default_value("0")
                        .help("Don't process slots lower than this value"),
                )
                .arg(
                    Arg::with_name("batch_size")
                        .long("--batch-size")
                        .value_name("NUMBER")
                        .takes_value(true)
                        .default_value("10")
                        .help("Process up to this many slots in one batch"),
                ),
        )
        .subcommand(
            SubCommand::with_name("check")
                .about("Check if a state account is in compliance")
                .arg(
                    Arg::with_name("stake_account_pubkey")
                        .index(1)
                        .value_name("ADDRESS")
                        .validator(is_pubkey)
                        .required(true)
                        .help("Stake account address"),
                ),
        )
        .get_matches();

    let data_file = value_t_or_exit!(matches, "data_file", String);

    match matches.subcommand() {
        ("record", Some(matches)) => {
            let batch_size = value_t_or_exit!(matches, "batch_size", u64);
            let first_slot = value_t_or_exit!(matches, "first_slot", Slot);
            let json_rpc_url = value_t!(matches, "json_rpc_url", String).unwrap_or_else(|_| {
                let config = if let Some(config_file) = matches.value_of("config_file") {
                    solana_cli_config::Config::load(config_file).unwrap_or_default()
                } else {
                    solana_cli_config::Config::default()
                };
                config.json_rpc_url
            });
            command_record(data_file, json_rpc_url, first_slot, batch_size);
        }
        ("check", Some(matches)) => {
            let stake_account_pubkey = pubkey_of(&matches, "stake_account_pubkey").unwrap();
            command_check(data_file, stake_account_pubkey);
        }
        _ => unreachable!(),
    }
}
