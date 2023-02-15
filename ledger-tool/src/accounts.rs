//! The `accounts_tool` subcommand
use {
    crate::utils::*,
    clap::{value_t, App, AppSettings, Arg, ArgMatches, SubCommand},
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

pub trait AccountsSubCommand<'a> {
    #[allow(clippy::too_many_arguments)]
    fn accounts_subcommand(
        self,
        account_paths_arg: &'a Arg,
        accounts_data_encoding_arg: &'a Arg,
        accounts_db_skip_initial_hash_calc_arg: &'a Arg,
        accounts_index_bins: &'a Arg,
        accounts_index_limit: &'a Arg,
        accountsdb_verify_refcounts: &'a Arg,
        disable_disk_index: &'a Arg,
        geyser_plugin_args: &'a Arg,
        halt_at_slot_arg: &'a Arg,
        hard_forks_arg: &'a Arg,
        max_genesis_archive_unpacked_size_arg: &'a Arg,
        no_snapshot_arg: &'a Arg,
    ) -> Self;
}

impl<'a> AccountsSubCommand<'a> for App<'a, '_> {
    #[allow(clippy::too_many_arguments)]
    fn accounts_subcommand(
        self,
        account_paths_arg: &'a Arg,
        accounts_data_encoding_arg: &'a Arg,
        accounts_db_skip_initial_hash_calc_arg: &'a Arg,
        accounts_index_bins: &'a Arg,
        accounts_index_limit: &'a Arg,
        accountsdb_verify_refcounts: &'a Arg,
        disable_disk_index: &'a Arg,
        geyser_plugin_args: &'a Arg,
        halt_at_slot_arg: &'a Arg,
        hard_forks_arg: &'a Arg,
        max_genesis_archive_unpacked_size_arg: &'a Arg,
        no_snapshot_arg: &'a Arg,
    ) -> Self {
        self.subcommand(
            SubCommand::with_name("accounts")
            .about("Print account stats and contents after processing the ledger")
            .setting(AppSettings::InferSubcommands)
            .arg(no_snapshot_arg)
            .arg(account_paths_arg)
            .arg(accounts_index_bins)
            .arg(accounts_index_limit)
            .arg(disable_disk_index)
            .arg(accountsdb_verify_refcounts)
            .arg(accounts_db_skip_initial_hash_calc_arg)
            .arg(halt_at_slot_arg)
            .arg(hard_forks_arg)
            .arg(geyser_plugin_args)
            .arg(accounts_data_encoding_arg)
            .arg(
                Arg::with_name("include_sysvars")
                    .long("include-sysvars")
                    .takes_value(false)
                    .help("Include sysvars too"),
            )
            .arg(
                Arg::with_name("no_account_contents")
                    .long("no-account-contents")
                    .takes_value(false)
                    .help("Do not print contents of each account, which is very slow with lots of accounts."),
            )
            .arg(Arg::with_name("no_account_data")
                .long("no-account-data")
                .takes_value(false)
                .help("Do not print account data when printing account contents."),
            )
            .arg(max_genesis_archive_unpacked_size_arg)
        )
    }
}

pub fn accounts_process_command(
    ledger_path: &Path,
    force_update_to_open: bool,
    wal_recovery_mode: Option<BlockstoreRecoveryMode>,
    snapshot_archive_path: Option<PathBuf>,
    incremental_snapshot_archive_path: Option<PathBuf>,
    arg_matches: &ArgMatches<'_>,
) {
    let halt_at_slot = value_t!(arg_matches, "halt_at_slot", Slot).ok();
    let process_options = ProcessOptions {
        new_hard_forks: hardforks_of(arg_matches, "hard_forks"),
        halt_at_slot,
        run_verification: false,
        accounts_db_config: Some(get_accounts_db_config(ledger_path, arg_matches)),
        ..ProcessOptions::default()
    };
    let genesis_config = open_genesis_config_by(ledger_path, arg_matches);
    let include_sysvars = arg_matches.is_present("include_sysvars");
    let blockstore = open_blockstore(
        ledger_path,
        AccessType::Secondary,
        wal_recovery_mode,
        force_update_to_open,
    );
    let (bank_forks, ..) = load_bank_forks(
        arg_matches,
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
        match OutputFormat::from_matches(arg_matches, "output_format", false) {
            OutputFormat::Json | OutputFormat::JsonCompact => {
                (false, Some(serializer.serialize_seq(None).unwrap()))
            }
            _ => (true, None),
        };
    let mut total_accounts_stats = TotalAccountsStats::default();
    let rent_collector = bank.rent_collector();
    let print_account_contents = !arg_matches.is_present("no_account_contents");
    let print_account_data = !arg_matches.is_present("no_account_data");
    let data_encoding = parse_encoding_format(arg_matches);
    let cli_account_new_config = CliAccountNewConfig {
        data_encoding,
        ..CliAccountNewConfig::default()
    };
    let scan_func = |some_account_tuple: Option<(&Pubkey, AccountSharedData, Slot)>| {
        if let Some((pubkey, account, slot)) =
            some_account_tuple.filter(|(_, account, _)| Accounts::is_loadable(account.lamports()))
        {
            if !include_sysvars && solana_sdk::sysvar::is_sysvar_id(pubkey) {
                return;
            }

            total_accounts_stats.accumulate_account(pubkey, &account, rent_collector);

            if print_account_contents {
                if let Some(json_serializer) = json_serializer.as_mut() {
                    let cli_account =
                        CliAccount::new_with_config(pubkey, &account, &cli_account_new_config);
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
