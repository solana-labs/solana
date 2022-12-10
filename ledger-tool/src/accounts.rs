//! The `accounts_tool` subcommand
use {
    crate::utils::*,
    clap::{value_t, value_t_or_exit, App, AppSettings, Arg, ArgMatches, SubCommand},
    log::*,
    serde::ser::{SerializeSeq, Serializer},
    solana_cli_output::{CliAccount, CliAccountNewConfig, OutputFormat},
    solana_ledger::{
        blockstore_options::{AccessType, BlockstoreRecoveryMode},
        blockstore_processor::ProcessOptions,
    },
    solana_measure::measure::Measure,
    solana_runtime::{accounts::Accounts, accounts_file::AccountsFile, bank::TotalAccountsStats},
    solana_sdk::{
        account::{AccountSharedData, ReadableAccount},
        clock::Slot,
        pubkey::Pubkey,
    },
    std::{
        cmp::Reverse,
        collections::{BinaryHeap, HashMap},
        io::stdout,
        path::{Path, PathBuf},
        process::exit,
        sync::Arc,
    },
};

#[derive(Debug, Eq)]
struct TopAccountsStatsEntry<K, V: std::cmp::Ord> {
    key: K,
    value: V,
}

impl<K: std::cmp::Eq, V: std::cmp::Ord> Ord for TopAccountsStatsEntry<K, V> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.value.cmp(&other.value)
    }
}

impl<K: std::cmp::Eq, V: std::cmp::Ord> PartialOrd for TopAccountsStatsEntry<K, V> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl<K: std::cmp::Eq, V: std::cmp::Ord> PartialEq for TopAccountsStatsEntry<K, V> {
    fn eq(&self, other: &Self) -> bool {
        self.value == other.value
    }
}

#[derive(PartialEq)]
#[allow(clippy::enum_variant_names)]
enum TopAccountsRankingField {
    DataSize,
    ExecutableDataSize,
    NonExecutableDataSize,
}

impl TopAccountsRankingField {
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "data_size" => Some(Self::DataSize),
            "exec_data_size" => Some(Self::ExecutableDataSize),
            "non_exec_data_size" => Some(Self::NonExecutableDataSize),
            _ => None,
        }
    }
}

#[derive(PartialEq)]
enum TopCommonValueRankingField {
    Owner,
}

impl TopCommonValueRankingField {
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "owner" => Some(Self::Owner),
            _ => None,
        }
    }
}

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
        default_top_accounts_file_count_limit: &'a str,
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
        default_top_accounts_file_count_limit: &'a str,
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
            .subcommand(
                SubCommand::with_name("top")
                .about("Print the top N accounts of the specified field.")
                .arg(
                    Arg::with_name("n")
                        .takes_value(true)
                        .value_name("N")
                        .default_value("30")
                        .help("Collect the top N entries of the specified --field")
                )
                .arg(
                    Arg::with_name("path")
                        .long("path")
                        .takes_value(true)
                        .value_name("ACCOUNTS_DB_PATH")
                        .help("Path to the accounts_db.")
                )
                .arg(
                    Arg::with_name("field")
                        .long("field")
                        .takes_value(true)
                        .value_name("FIELD")
                        .possible_values(&["data_size", "exec_data_size", "non_exec_data_size"])
                        .default_value("data_size")
                        .help("Determine which stats to print. \
                               Possible values are: \
                               'data_size': print the top N accounts ranked by the account data size. \
                               'exec_data_size': print the top N accounts ranked by the executable account data size. \
                               'non_exec_data_size': print the top N accounts ranked by the non-executable account data size.")
                )
                .arg(
                    Arg::with_name("limit")
                        .long("limit")
                        .takes_value(true)
                        .value_name("LIMIT")
                        .default_value(default_top_accounts_file_count_limit)
                        .help("Collect stats from up to LIMIT accounts db files.")
                )
                .subcommand(
                    SubCommand::with_name("top-values")
                    .about("Print the top N common values in the account db.")
                    .arg(
                        Arg::with_name("path")
                            .long("path")
                            .takes_value(true)
                            .value_name("ACCOUNTS_DB_PATH")
                            .required(true)
                            .help("Path to the accounts_db.")
                    )
                    .arg(
                        Arg::with_name("field")
                            .long("field")
                            .takes_value(true)
                            .value_name("FIELD")
                            .possible_values(&["owner"])
                            .default_value("owner")
                            .required(true)
                            .help("Determine which stats to print. \
                                   Possible values are: \
                                   'owner': print the top N common owners from all accounts.")
                    )
                    .arg(
                        Arg::with_name("n")
                            .takes_value(true)
                            .value_name("N")
                            .default_value("30")
                            .help("Collect the top N entries of the specified --stats")
                    )
                    .arg(
                        Arg::with_name("limit")
                            .long("limit")
                            .takes_value(true)
                            .value_name("LIMIT")
                            .default_value(default_top_accounts_file_count_limit)
                            .help("Collect stats from up to LIMIT accounts db files.")
                    )
                )
            )
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
    let (subcommand, sub_matches) = arg_matches.subcommand();
    match (subcommand, sub_matches) {
        ("top", Some(arg_matches)) => {
            let accounts_db_path = value_t_or_exit!(arg_matches, "path", String);
            let acc_file_paths = std::fs::read_dir(accounts_db_path).unwrap();
            let heap_size = value_t_or_exit!(arg_matches, "n", usize);
            let limit = value_t_or_exit!(arg_matches, "limit", usize);
            let field_str = value_t_or_exit!(arg_matches, "field", String);
            let field = TopAccountsRankingField::from_str(&field_str).unwrap();

            let mut min_heap = BinaryHeap::new();
            let mut file_count = 0;
            debug!("paths = {:?}", acc_file_paths);
            for path in acc_file_paths {
                debug!("Collecting stats from {:?}", path);
                let av_path = path.expect("success").path();
                let av_len = std::fs::metadata(&av_path).unwrap().len() as usize;
                if let Ok((mut acc_file, _)) = AccountsFile::new_from_file(av_path, av_len) {
                    acc_file.set_no_remove_on_drop();

                    // read append-vec
                    let mut offset = 0;
                    while let Some((account, next_offset)) = acc_file.get_account(offset) {
                        offset = next_offset;
                        // data_size
                        min_heap.push(Reverse(TopAccountsStatsEntry {
                            key: *account.pubkey(),
                            value: match field {
                                TopAccountsRankingField::DataSize => account.data_len(),
                                TopAccountsRankingField::ExecutableDataSize => {
                                    if account.executable() {
                                        account.data_len()
                                    } else {
                                        0
                                    }
                                }
                                TopAccountsRankingField::NonExecutableDataSize => {
                                    if account.executable() {
                                        0
                                    } else {
                                        account.data_len()
                                    }
                                }
                            },
                        }));
                        if min_heap.len() > heap_size {
                            min_heap.pop();
                        }
                    }
                    file_count += 1;
                    if file_count >= limit {
                        break;
                    }
                }
            }
            println!("Collected top {:?} samples", min_heap.len());
            while !min_heap.is_empty() {
                if let Some(Reverse(entry)) = min_heap.pop() {
                    println!("account: {:?}, {}: {:?}", entry.key, field_str, entry.value);
                }
            }
        }
        ("top-values", Some(arg_matches)) => {
            let accounts_db_path = value_t_or_exit!(arg_matches, "path", String);
            let acc_file_paths = std::fs::read_dir(accounts_db_path).unwrap();
            let heap_size = value_t_or_exit!(arg_matches, "n", usize);
            let limit = value_t_or_exit!(arg_matches, "limit", usize);
            let field_str = &value_t_or_exit!(arg_matches, "field", String);
            let field = TopCommonValueRankingField::from_str(field_str).unwrap();

            let mut file_count = 0;
            let mut hash_map: HashMap<String, usize> = HashMap::new();
            debug!("paths = {:?}", acc_file_paths);
            for path in acc_file_paths {
                debug!("Collecting stats from {:?}", path);
                let av_path = path.expect("success").path();
                let av_len = std::fs::metadata(&av_path).unwrap().len() as usize;
                if let Ok((mut acc_file, _)) = AccountsFile::new_from_file(av_path, av_len) {
                    acc_file.set_no_remove_on_drop();

                    // read append-vec
                    let mut offset = 0;
                    while let Some((account, next_offset)) = acc_file.get_account(offset) {
                        offset = next_offset;
                        let key = match field {
                            TopCommonValueRankingField::Owner => account.owner().to_string(),
                        };
                        if let Some(count_entry) = hash_map.get_mut(&key) {
                            *count_entry += 1;
                        } else {
                            hash_map.insert(key, 1);
                        }
                    }
                    file_count += 1;
                    if file_count >= limit {
                        break;
                    }
                }
            }

            let mut min_heap = BinaryHeap::new();
            for (k, v) in hash_map {
                min_heap.push(Reverse(TopAccountsStatsEntry { key: k, value: v }));
                if min_heap.len() > heap_size {
                    min_heap.pop();
                }
            }
            println!("Collected top {:?} samples", min_heap.len());
            while !min_heap.is_empty() {
                if let Some(Reverse(entry)) = min_heap.pop() {
                    println!("{}: {:?}, count: {:?}", field_str, entry.key, entry.value);
                }
            }
        }
        _ => {
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
