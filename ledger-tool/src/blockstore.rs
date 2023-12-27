//! The `blockstore` subcommand

use {
    crate::{
        ledger_path::canonicalize_ledger_path,
        output::{SlotBounds, SlotInfo},
    },
    clap::{
        value_t, value_t_or_exit, values_t_or_exit, App, AppSettings, Arg, ArgMatches, SubCommand,
    },
    serde_json::json,
    solana_clap_utils::input_validators::is_slot,
    solana_cli_output::OutputFormat,
    solana_ledger::{
        blockstore::Blockstore,
        blockstore_db::{self, Column, ColumnName, Database},
        blockstore_options::AccessType,
    },
    solana_sdk::clock::Slot,
    std::{
        fs::File,
        io::{stdout, Write},
        path::Path,
        sync::atomic::AtomicBool,
    },
};

fn analyze_column<
    C: solana_ledger::blockstore_db::Column + solana_ledger::blockstore_db::ColumnName,
>(
    db: &Database,
    name: &str,
) {
    let mut key_len: u64 = 0;
    let mut key_tot: u64 = 0;
    let mut val_hist = histogram::Histogram::new();
    let mut val_tot: u64 = 0;
    let mut row_hist = histogram::Histogram::new();
    for (key, val) in db.iter::<C>(blockstore_db::IteratorMode::Start).unwrap() {
        // Key length is fixed, only need to calculate it once
        if key_len == 0 {
            key_len = C::key(key).len() as u64;
        }
        let val_len = val.len() as u64;

        key_tot += key_len;
        val_hist.increment(val_len).unwrap();
        val_tot += val_len;

        row_hist.increment(key_len + val_len).unwrap();
    }

    let json_result = if val_hist.entries() > 0 {
        json!({
            "column":name,
            "entries":val_hist.entries(),
            "key_stats":{
                "max":key_len,
                "total_bytes":key_tot,
            },
            "val_stats":{
                "p50":val_hist.percentile(50.0).unwrap(),
                "p90":val_hist.percentile(90.0).unwrap(),
                "p99":val_hist.percentile(99.0).unwrap(),
                "p999":val_hist.percentile(99.9).unwrap(),
                "min":val_hist.minimum().unwrap(),
                "max":val_hist.maximum().unwrap(),
                "stddev":val_hist.stddev().unwrap(),
                "total_bytes":val_tot,
            },
            "row_stats":{
                "p50":row_hist.percentile(50.0).unwrap(),
                "p90":row_hist.percentile(90.0).unwrap(),
                "p99":row_hist.percentile(99.0).unwrap(),
                "p999":row_hist.percentile(99.9).unwrap(),
                "min":row_hist.minimum().unwrap(),
                "max":row_hist.maximum().unwrap(),
                "stddev":row_hist.stddev().unwrap(),
                "total_bytes":key_tot + val_tot,
            },
        })
    } else {
        json!({
        "column":name,
        "entries":val_hist.entries(),
        "key_stats":{
            "max":key_len,
            "total_bytes":0,
        },
        "val_stats":{
            "total_bytes":0,
        },
        "row_stats":{
            "total_bytes":0,
        },
        })
    };

    println!("{}", serde_json::to_string_pretty(&json_result).unwrap());
}

fn analyze_storage(database: &Database) {
    use solana_ledger::blockstore_db::columns::*;
    analyze_column::<SlotMeta>(database, "SlotMeta");
    analyze_column::<Orphans>(database, "Orphans");
    analyze_column::<DeadSlots>(database, "DeadSlots");
    analyze_column::<DuplicateSlots>(database, "DuplicateSlots");
    analyze_column::<ErasureMeta>(database, "ErasureMeta");
    analyze_column::<BankHash>(database, "BankHash");
    analyze_column::<Root>(database, "Root");
    analyze_column::<Index>(database, "Index");
    analyze_column::<ShredData>(database, "ShredData");
    analyze_column::<ShredCode>(database, "ShredCode");
    analyze_column::<TransactionStatus>(database, "TransactionStatus");
    analyze_column::<AddressSignatures>(database, "AddressSignatures");
    analyze_column::<TransactionMemos>(database, "TransactionMemos");
    analyze_column::<TransactionStatusIndex>(database, "TransactionStatusIndex");
    analyze_column::<Rewards>(database, "Rewards");
    analyze_column::<Blocktime>(database, "Blocktime");
    analyze_column::<PerfSamples>(database, "PerfSamples");
    analyze_column::<BlockHeight>(database, "BlockHeight");
    analyze_column::<ProgramCosts>(database, "ProgramCosts");
    analyze_column::<OptimisticSlots>(database, "OptimisticSlots");
}

fn raw_key_to_slot(key: &[u8], column_name: &str) -> Option<Slot> {
    use solana_ledger::blockstore_db::columns as cf;
    match column_name {
        cf::SlotMeta::NAME => Some(cf::SlotMeta::slot(cf::SlotMeta::index(key))),
        cf::Orphans::NAME => Some(cf::Orphans::slot(cf::Orphans::index(key))),
        cf::DeadSlots::NAME => Some(cf::SlotMeta::slot(cf::SlotMeta::index(key))),
        cf::DuplicateSlots::NAME => Some(cf::SlotMeta::slot(cf::SlotMeta::index(key))),
        cf::ErasureMeta::NAME => Some(cf::ErasureMeta::slot(cf::ErasureMeta::index(key))),
        cf::BankHash::NAME => Some(cf::BankHash::slot(cf::BankHash::index(key))),
        cf::Root::NAME => Some(cf::Root::slot(cf::Root::index(key))),
        cf::Index::NAME => Some(cf::Index::slot(cf::Index::index(key))),
        cf::ShredData::NAME => Some(cf::ShredData::slot(cf::ShredData::index(key))),
        cf::ShredCode::NAME => Some(cf::ShredCode::slot(cf::ShredCode::index(key))),
        cf::TransactionStatus::NAME => Some(cf::TransactionStatus::slot(
            cf::TransactionStatus::index(key),
        )),
        cf::AddressSignatures::NAME => Some(cf::AddressSignatures::slot(
            cf::AddressSignatures::index(key),
        )),
        cf::TransactionMemos::NAME => None, // does not implement slot()
        cf::TransactionStatusIndex::NAME => None, // does not implement slot()
        cf::Rewards::NAME => Some(cf::Rewards::slot(cf::Rewards::index(key))),
        cf::Blocktime::NAME => Some(cf::Blocktime::slot(cf::Blocktime::index(key))),
        cf::PerfSamples::NAME => Some(cf::PerfSamples::slot(cf::PerfSamples::index(key))),
        cf::BlockHeight::NAME => Some(cf::BlockHeight::slot(cf::BlockHeight::index(key))),
        cf::ProgramCosts::NAME => None, // does not implement slot()
        cf::OptimisticSlots::NAME => {
            Some(cf::OptimisticSlots::slot(cf::OptimisticSlots::index(key)))
        }
        &_ => None,
    }
}

fn print_blockstore_file_metadata(
    blockstore: &Blockstore,
    file_name: &Option<&str>,
) -> Result<(), String> {
    let live_files = blockstore
        .live_files_metadata()
        .map_err(|err| format!("{err:?}"))?;

    // All files under live_files_metadata are prefixed with "/".
    let sst_file_name = file_name.as_ref().map(|name| format!("/{name}"));
    for file in live_files {
        if sst_file_name.is_none() || file.name.eq(sst_file_name.as_ref().unwrap()) {
            println!(
                "[{}] cf_name: {}, level: {}, start_slot: {:?}, end_slot: {:?}, size: {}, \
                 num_entries: {}",
                file.name,
                file.column_family_name,
                file.level,
                raw_key_to_slot(&file.start_key.unwrap(), &file.column_family_name),
                raw_key_to_slot(&file.end_key.unwrap(), &file.column_family_name),
                file.size,
                file.num_entries,
            );
            if sst_file_name.is_some() {
                return Ok(());
            }
        }
    }
    if sst_file_name.is_some() {
        return Err(format!(
            "Failed to find or load the metadata of the specified file {file_name:?}"
        ));
    }
    Ok(())
}

pub trait BlockstoreSubCommand {
    fn blockstore_subcommand(self) -> Self;
}

impl BlockstoreSubCommand for App<'_, '_> {
    fn blockstore_subcommand(self) -> Self {
        self.subcommand(
            SubCommand::with_name("blockstore")
                .about("Commands to interact with a local Blockstore")
                .setting(AppSettings::InferSubcommands)
                .setting(AppSettings::SubcommandRequiredElseHelp)
                .subcommands(blockstore_subcommands(false)),
        )
    }
}

pub fn blockstore_subcommands<'a, 'b>(hidden: bool) -> Vec<App<'a, 'b>> {
    let hidden = if hidden {
        vec![AppSettings::Hidden]
    } else {
        vec![]
    };

    let starting_slot_arg = Arg::with_name("starting_slot")
        .long("starting-slot")
        .value_name("SLOT")
        .takes_value(true)
        .default_value("0")
        .help("Start at this slot");

    vec![
        SubCommand::with_name("analyze-storage")
            .about(
                "Output statistics in JSON format about all column families in the ledger \
                rocksdb",
            )
            .settings(&hidden),
        SubCommand::with_name("bounds")
            .about(
                "Print lowest and highest non-empty slots. Note that there may be empty slots \
                 within the bounds",
            )
            .settings(&hidden)
            .arg(
                Arg::with_name("all")
                    .long("all")
                    .takes_value(false)
                    .required(false)
                    .help("Additionally print all the non-empty slots within the bounds"),
            ),
        SubCommand::with_name("dead-slots")
            .about("Print all the dead slots in the ledger")
            .settings(&hidden)
            .arg(&starting_slot_arg),
        SubCommand::with_name("duplicate-slots")
            .about("Print all the duplicate slots in the ledger")
            .settings(&hidden)
            .arg(&starting_slot_arg),
        SubCommand::with_name("list-roots")
            .about(
                "Output up to last <num-roots> root hashes and their heights starting at the \
                 given block height",
            )
            .settings(&hidden)
            .arg(
                Arg::with_name("max_height")
                    .long("max-height")
                    .value_name("NUM")
                    .takes_value(true)
                    .help("Maximum block height"),
            )
            .arg(
                Arg::with_name("start_root")
                    .long("start-root")
                    .value_name("NUM")
                    .takes_value(true)
                    .help("First root to start searching from"),
            )
            .arg(
                Arg::with_name("slot_list")
                    .long("slot-list")
                    .value_name("FILENAME")
                    .required(false)
                    .takes_value(true)
                    .help(
                        "The location of the output YAML file. A list of rollback slot \
                         heights and hashes will be written to the file",
                    ),
            )
            .arg(
                Arg::with_name("num_roots")
                    .long("num-roots")
                    .value_name("NUM")
                    .takes_value(true)
                    .default_value("1")
                    .required(false)
                    .help("Number of roots in the output"),
            ),
        SubCommand::with_name("print-file-metadata")
            .about(
                "Print the metadata of the specified ledger-store file. If no file name is \
                 specified, it will print the metadata of all ledger files.",
            )
            .settings(&hidden)
            .arg(
                Arg::with_name("file_name")
                    .long("file-name")
                    .takes_value(true)
                    .value_name("SST_FILE_NAME")
                    .help(
                        "The ledger file name (e.g. 011080.sst.) If no file name is \
                         specified, it will print the metadata of all ledger files.",
                    ),
            ),
        SubCommand::with_name("remove-dead-slot")
            .about("Remove the dead flag for a slot")
            .settings(&hidden)
            .arg(
                Arg::with_name("slots")
                    .index(1)
                    .value_name("SLOTS")
                    .validator(is_slot)
                    .takes_value(true)
                    .multiple(true)
                    .required(true)
                    .help("Slots to mark as not dead"),
            ),
        SubCommand::with_name("repair-roots")
            .about(
                "Traverses the AncestorIterator backward from a last known root to restore \
                 missing roots to the Root column",
            )
            .settings(&hidden)
            .arg(
                Arg::with_name("start_root")
                    .long("before")
                    .value_name("NUM")
                    .takes_value(true)
                    .help("Recent root after the range to repair"),
            )
            .arg(
                Arg::with_name("end_root")
                    .long("until")
                    .value_name("NUM")
                    .takes_value(true)
                    .help("Earliest slot to check for root repair"),
            )
            .arg(
                Arg::with_name("max_slots")
                    .long("repair-limit")
                    .value_name("NUM")
                    .takes_value(true)
                    .default_value("2000")
                    .required(true)
                    .help("Override the maximum number of slots to check for root repair"),
            ),
        SubCommand::with_name("set-dead-slot")
            .about("Mark one or more slots dead")
            .settings(&hidden)
            .arg(
                Arg::with_name("slots")
                    .index(1)
                    .value_name("SLOTS")
                    .validator(is_slot)
                    .takes_value(true)
                    .multiple(true)
                    .required(true)
                    .help("Slots to mark dead"),
            ),
    ]
}

pub fn blockstore_process_command(ledger_path: &Path, matches: &ArgMatches<'_>) {
    let ledger_path = canonicalize_ledger_path(ledger_path);

    match matches.subcommand() {
        ("analyze-storage", Some(arg_matches)) => {
            analyze_storage(
                &crate::open_blockstore(&ledger_path, arg_matches, AccessType::Secondary).db(),
            );
        }
        ("bounds", Some(arg_matches)) => {
            let blockstore =
                crate::open_blockstore(&ledger_path, arg_matches, AccessType::Secondary);

            match blockstore.slot_meta_iterator(0) {
                Ok(metas) => {
                    let output_format =
                        OutputFormat::from_matches(arg_matches, "output_format", false);
                    let all = arg_matches.is_present("all");

                    let slots: Vec<_> = metas.map(|(slot, _)| slot).collect();

                    let slot_bounds = if slots.is_empty() {
                        SlotBounds::default()
                    } else {
                        // Collect info about slot bounds
                        let mut bounds = SlotBounds {
                            slots: SlotInfo {
                                total: slots.len(),
                                first: Some(*slots.first().unwrap()),
                                last: Some(*slots.last().unwrap()),
                                ..SlotInfo::default()
                            },
                            ..SlotBounds::default()
                        };
                        if all {
                            bounds.all_slots = Some(&slots);
                        }

                        // Consider also rooted slots, if present
                        if let Ok(rooted) = blockstore.rooted_slot_iterator(0) {
                            let mut first_rooted = None;
                            let mut last_rooted = None;
                            let mut total_rooted = 0;
                            for (i, slot) in rooted.into_iter().enumerate() {
                                if i == 0 {
                                    first_rooted = Some(slot);
                                }
                                last_rooted = Some(slot);
                                total_rooted += 1;
                            }
                            let last_root_for_comparison = last_rooted.unwrap_or_default();
                            let count_past_root = slots
                                .iter()
                                .rev()
                                .take_while(|slot| *slot > &last_root_for_comparison)
                                .count();

                            bounds.roots = SlotInfo {
                                total: total_rooted,
                                first: first_rooted,
                                last: last_rooted,
                                num_after_last_root: Some(count_past_root),
                            };
                        }
                        bounds
                    };

                    // Print collected data
                    println!("{}", output_format.formatted_string(&slot_bounds));
                }
                Err(err) => {
                    eprintln!("Unable to read the Ledger: {err:?}");
                    std::process::exit(1);
                }
            };
        }
        ("dead-slots", Some(arg_matches)) => {
            let blockstore =
                crate::open_blockstore(&ledger_path, arg_matches, AccessType::Secondary);
            let starting_slot = value_t_or_exit!(arg_matches, "starting_slot", Slot);
            for slot in blockstore.dead_slots_iterator(starting_slot).unwrap() {
                println!("{slot}");
            }
        }
        ("duplicate-slots", Some(arg_matches)) => {
            let blockstore =
                crate::open_blockstore(&ledger_path, arg_matches, AccessType::Secondary);
            let starting_slot = value_t_or_exit!(arg_matches, "starting_slot", Slot);
            for slot in blockstore.duplicate_slots_iterator(starting_slot).unwrap() {
                println!("{slot}");
            }
        }
        ("list-roots", Some(arg_matches)) => {
            let blockstore =
                crate::open_blockstore(&ledger_path, arg_matches, AccessType::Secondary);

            let max_height = value_t!(arg_matches, "max_height", usize).unwrap_or(usize::MAX);
            let start_root = value_t!(arg_matches, "start_root", Slot).unwrap_or(0);
            let num_roots = value_t_or_exit!(arg_matches, "num_roots", usize);

            let iter = blockstore
                .rooted_slot_iterator(start_root)
                .expect("Failed to get rooted slot");

            let mut output: Box<dyn Write> = if let Some(path) = arg_matches.value_of("slot_list") {
                match File::create(path) {
                    Ok(file) => Box::new(file),
                    _ => Box::new(stdout()),
                }
            } else {
                Box::new(stdout())
            };

            iter.take(num_roots)
                .take_while(|slot| *slot <= max_height as u64)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .for_each(|slot| {
                    let blockhash = blockstore
                        .get_slot_entries(slot, 0)
                        .unwrap()
                        .last()
                        .unwrap()
                        .hash;

                    writeln!(output, "{slot}: {blockhash:?}").expect("failed to write");
                });
        }
        ("print-file-metadata", Some(arg_matches)) => {
            let blockstore =
                crate::open_blockstore(&ledger_path, arg_matches, AccessType::Secondary);
            let sst_file_name = arg_matches.value_of("file_name");
            if let Err(err) = print_blockstore_file_metadata(&blockstore, &sst_file_name) {
                eprintln!("{err}");
            }
        }
        ("remove-dead-slot", Some(arg_matches)) => {
            let slots = values_t_or_exit!(arg_matches, "slots", Slot);
            let blockstore = crate::open_blockstore(&ledger_path, arg_matches, AccessType::Primary);
            for slot in slots {
                match blockstore.remove_dead_slot(slot) {
                    Ok(_) => println!("Slot {slot} not longer marked dead"),
                    Err(err) => {
                        eprintln!("Failed to remove dead flag for slot {slot}, {err:?}")
                    }
                }
            }
        }
        ("repair-roots", Some(arg_matches)) => {
            let blockstore = crate::open_blockstore(&ledger_path, arg_matches, AccessType::Primary);

            let start_root =
                value_t!(arg_matches, "start_root", Slot).unwrap_or_else(|_| blockstore.max_root());
            let max_slots = value_t_or_exit!(arg_matches, "max_slots", u64);
            let end_root = value_t!(arg_matches, "end_root", Slot)
                .unwrap_or_else(|_| start_root.saturating_sub(max_slots));
            assert!(start_root > end_root);
            let num_slots = start_root - end_root - 1; // Adjust by one since start_root need not be checked
            if arg_matches.is_present("end_root") && num_slots > max_slots {
                eprintln!(
                    "Requested range {num_slots} too large, max {max_slots}. Either adjust \
                 `--until` value, or pass a larger `--repair-limit` to override the limit",
                );
                std::process::exit(1);
            }

            let num_repaired_roots = blockstore
                .scan_and_fix_roots(Some(start_root), Some(end_root), &AtomicBool::new(false))
                .unwrap_or_else(|err| {
                    eprintln!("Unable to repair roots: {err}");
                    std::process::exit(1);
                });
            println!("Successfully repaired {num_repaired_roots} roots");
        }
        ("set-dead-slot", Some(arg_matches)) => {
            let slots = values_t_or_exit!(arg_matches, "slots", Slot);
            let blockstore = crate::open_blockstore(&ledger_path, arg_matches, AccessType::Primary);
            for slot in slots {
                match blockstore.set_dead_slot(slot) {
                    Ok(_) => println!("Slot {slot} dead"),
                    Err(err) => eprintln!("Failed to set slot {slot} dead slot: {err:?}"),
                }
            }
        }
        _ => unreachable!(),
    }
}
