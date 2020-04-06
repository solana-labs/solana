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
use solana_sdk::{clock::Slot, native_token::lamports_to_sol, pubkey::Pubkey, system_program};
use solana_stake_monitor::*;
use std::{fs, io, process};

fn load_accounts_info(data_file: &str) -> AccountsInfo {
    let data_file_new = data_file.to_owned() + "new";
    let accounts_info = solana_cli_config::load_config_file(&data_file_new)
        .or_else(|_| solana_cli_config::load_config_file(data_file))
        .unwrap_or_default();

    // Ensure `data_file` always exists
    save_accounts_info(data_file, &accounts_info).expect("save_accounts_info");

    accounts_info
}

fn save_accounts_info(data_file: &str, accounts_info: &AccountsInfo) -> io::Result<()> {
    let data_file_new = data_file.to_owned() + "new";
    solana_cli_config::save_config_file(&accounts_info, &data_file_new)?;
    let _ = fs::remove_file(data_file);
    fs::rename(&data_file_new, data_file)
}

fn command_record(data_file: &str, json_rpc_url: String, first_slot: Slot, batch_size: u64) {
    let mut accounts_info = load_accounts_info(&data_file);

    info!("RPC URL: {}", json_rpc_url);
    let rpc_client = RpcClient::new(json_rpc_url);
    if accounts_info.slot < first_slot {
        accounts_info.slot = first_slot;
    }

    loop {
        process_slots(&rpc_client, &mut accounts_info, batch_size);
        save_accounts_info(data_file, &accounts_info).unwrap_or_else(|err| {
            datapoint_error!(
                "stake-monitor-failure",
                (
                    "err",
                    format!("failed to save accounts_info: {}", err),
                    String
                )
            );
        });
    }
}

fn command_enroll(data_file: &str, json_rpc_url: String, account_address: &Pubkey) {
    info!("RPC URL: {}", json_rpc_url);
    let rpc_client = RpcClient::new(json_rpc_url);
    let slot = rpc_client.get_slot().expect("get slot");

    let account = rpc_client
        .get_account(account_address)
        .unwrap_or_else(|err| {
            eprintln!(
                "Unable to get account info for {}: {}",
                account_address, err
            );
            process::exit(1);
        });

    if account.owner != system_program::id() && !account.data.is_empty() {
        eprintln!("{} is not a system account", account_address);
        process::exit(1);
    }

    let mut accounts_info = load_accounts_info(data_file);
    accounts_info.enroll_system_account(account_address, slot, account.lamports);
    save_accounts_info(data_file, &accounts_info).unwrap();
    println!(
        "Enrolled {} at slot {} with a balance of {} SOL",
        account_address,
        slot,
        lamports_to_sol(account.lamports)
    );
}

fn command_check(data_file: &str, account_address: &Pubkey) {
    let accounts_info = load_accounts_info(data_file);

    if let Some(account_info) = accounts_info.account_info.get(&account_address.to_string()) {
        if let Some(slot) = account_info.compliant_since {
            println!(
                "{}Account compliant since slot {} with a balance of {} SOL",
                Emoji("✅ ", ""),
                slot,
                lamports_to_sol(account_info.lamports)
            );
            process::exit(0);
        } else {
            eprintln!(
                "{}Account not compliant due to: {:?}",
                Emoji("❌ ", ""),
                account_info.transactions.last().unwrap()
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
        .arg(
            Arg::with_name("json_rpc_url")
                .long("url")
                .value_name("URL")
                .takes_value(true)
                .validator(is_url)
                .help("JSON RPC URL for the cluster"),
        )
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
        .subcommand(
            SubCommand::with_name("record")
                .about("Monitor all Cluster transactions for state account compliance")
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
                .about("Check if an account is in compliance")
                .arg(
                    Arg::with_name("account_address")
                        .index(1)
                        .value_name("ADDRESS")
                        .validator(is_pubkey)
                        .required(true)
                        .help("Account address"),
                ),
        )
        .subcommand(
            SubCommand::with_name("enroll")
                .about("Enroll a system account for balance monitoring")
                .arg(
                    Arg::with_name("account_address")
                        .index(1)
                        .value_name("ADDRESS")
                        .validator(is_pubkey)
                        .required(true)
                        .help("Account address"),
                ),
        )
        .get_matches();

    let data_file = value_t_or_exit!(matches, "data_file", String);
    let json_rpc_url = value_t!(matches, "json_rpc_url", String).unwrap_or_else(|_| {
        let config = if let Some(config_file) = matches.value_of("config_file") {
            solana_cli_config::Config::load(config_file).unwrap_or_default()
        } else {
            solana_cli_config::Config::default()
        };
        config.json_rpc_url
    });

    match matches.subcommand() {
        ("record", Some(matches)) => {
            let batch_size = value_t_or_exit!(matches, "batch_size", u64);
            let first_slot = value_t_or_exit!(matches, "first_slot", Slot);
            command_record(&data_file, json_rpc_url, first_slot, batch_size);
        }
        ("check", Some(matches)) => {
            let account_address = pubkey_of(&matches, "account_address").unwrap();
            command_check(&data_file, &account_address);
        }
        ("enroll", Some(matches)) => {
            let account_address = pubkey_of(&matches, "account_address").unwrap();
            command_enroll(&data_file, json_rpc_url, &account_address);
        }
        _ => unreachable!(),
    }
}
