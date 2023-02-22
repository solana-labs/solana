//! The `accounts_tool` subcommand
use {
    crate::utils::*,
    clap::{value_t, App, AppSettings, ArgMatches, SubCommand},
    log::*,
    serde::ser::{SerializeSeq, Serializer},
    solana_cli_output::{CliAccount, CliAccountNewConfig, OutputFormat},
    solana_ledger::{
        blockstore_options::{AccessType, BlockstoreRecoveryMode},
        blockstore_processor::ProcessOptions,
    },
    solana_measure::measure::Measure,
    solana_runtime::{accounts::Accounts, bank::TotalAccountsStats},
    solana_sdk::{
        account::{AccountSharedData, ReadableAccount},
        clock::Slot,
        pubkey::Pubkey,
    },
    std::{
        io::stdout,
        path::{Path, PathBuf},
        process::exit,
        sync::Arc,
    },
};

pub trait AccountsSubCommand {
    fn accounts_subcommand(self) -> Self;
}

impl AccountsSubCommand for App<'_, '_> {
    fn accounts_subcommand(self) -> Self {
        self.subcommand(
            SubCommand::with_name("accounts")
                .about("Tool for analyzing accounts")
                .setting(AppSettings::InferSubcommands)
                .setting(AppSettings::SubcommandRequiredElseHelp),
        )
    }
}

pub fn accounts_process_command(
    ledger_path: &Path,
    force_update_to_open: bool,
    wal_recovery_mode: Option<BlockstoreRecoveryMode>,
    snapshot_archive_path: Option<PathBuf>,
    incremental_snapshot_archive_path: Option<PathBuf>,
    matches: &ArgMatches<'_>,
) {
    let (subcommand, sub_matches) = matches.subcommand();
    match (subcommand, sub_matches) {
        _ => {
            let halt_at_slot = value_t!(matches, "halt_at_slot", Slot).ok();
            let process_options = ProcessOptions {
                new_hard_forks: hardforks_of(matches, "hard_forks"),
                halt_at_slot,
                poh_verify: false,
                ..ProcessOptions::default()
            };
            let genesis_config = open_genesis_config_by(&ledger_path, matches);
            let include_sysvars = matches.is_present("include_sysvars");
            let blockstore = open_blockstore(
                &ledger_path,
                AccessType::Secondary,
                wal_recovery_mode,
                force_update_to_open,
            );
            let (bank_forks, ..) = load_bank_forks(
                matches,
                &genesis_config,
                Arc::new(blockstore),
                process_options,
                snapshot_archive_path,
                incremental_snapshot_archive_path,
            )
            .unwrap_or_else(|err| {
                eprintln!("Failed to load ledger: {err:?}");
                exit(1);
            });

            let bank = bank_forks.read().unwrap().working_bank();
            let mut serializer = serde_json::Serializer::new(stdout());
            let (summarize, mut json_serializer) =
                match OutputFormat::from_matches(matches, "output_format", false) {
                    OutputFormat::Json | OutputFormat::JsonCompact => {
                        (false, Some(serializer.serialize_seq(None).unwrap()))
                    }
                    _ => (true, None),
                };
            let mut total_accounts_stats = TotalAccountsStats::default();
            let rent_collector = bank.rent_collector();
            let print_account_contents = !matches.is_present("no_account_contents");
            let print_account_data = !matches.is_present("no_account_data");
            let data_encoding = parse_encoding_format(matches);
            let cli_account_new_config = CliAccountNewConfig {
                data_encoding,
                ..CliAccountNewConfig::default()
            };
            let scan_func = |some_account_tuple: Option<(&Pubkey, AccountSharedData, Slot)>| {
                if let Some((pubkey, account, slot)) = some_account_tuple
                    .filter(|(_, account, _)| Accounts::is_loadable(account.lamports()))
                {
                    if !include_sysvars && solana_sdk::sysvar::is_sysvar_id(pubkey) {
                        return;
                    }

                    total_accounts_stats.accumulate_account(pubkey, &account, rent_collector);

                    if print_account_contents {
                        if let Some(json_serializer) = json_serializer.as_mut() {
                            let cli_account = CliAccount::new_with_config(
                                pubkey,
                                &account,
                                &cli_account_new_config,
                            );
                            json_serializer.serialize_element(&cli_account).unwrap();
                        } else {
                            output_account(
                                pubkey,
                                &account,
                                Some(slot),
                                print_account_data,
                                data_encoding,
                            );
                        }
                    }
                }
            };
            let mut measure = Measure::start("scanning accounts");
            bank.scan_all_accounts_with_modified_slots(scan_func)
                .unwrap();
            measure.stop();
            info!("{}", measure);
            if let Some(json_serializer) = json_serializer {
                json_serializer.end().unwrap();
            }
            if summarize {
                println!("\n{total_accounts_stats:#?}");
            }
        }
    }
}
