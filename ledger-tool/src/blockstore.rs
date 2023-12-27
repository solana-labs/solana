//! The `blockstore` subcommand

use {
    crate::{
        ledger_path::canonicalize_ledger_path,
        ledger_utils::get_shred_storage_type,
        output::{SlotBounds, SlotInfo},
    },
    clap::{
        value_t, value_t_or_exit, values_t_or_exit, App, AppSettings, Arg, ArgMatches, SubCommand,
    },
    itertools::Itertools,
    log::*,
    serde_json::json,
    solana_clap_utils::{hidden_unless_forced, input_validators::is_slot},
    solana_cli_output::OutputFormat,
    solana_ledger::{
        blockstore::{Blockstore, PurgeType},
        blockstore_db::{self, Column, ColumnName, Database},
        blockstore_options::{AccessType, BLOCKSTORE_DIRECTORY_ROCKS_FIFO},
        shred::Shred,
    },
    solana_sdk::clock::Slot,
    std::{
        fs::File,
        io::{stdout, Write},
        path::{Path, PathBuf},
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
    let ending_slot_arg = Arg::with_name("ending_slot")
        .long("ending-slot")
        .value_name("SLOT")
        .takes_value(true)
        .help("The last slot to iterate to");

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
        SubCommand::with_name("copy")
            .about("Copy the ledger")
            .settings(&hidden)
            .arg(&starting_slot_arg)
            .arg(&ending_slot_arg)
            .arg(
                Arg::with_name("target_db")
                    .long("target-db")
                    .value_name("DIR")
                    .takes_value(true)
                    .help("Target db"),
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
        SubCommand::with_name("purge")
            .about("Delete a range of slots from the ledger")
            .settings(&hidden)
            .arg(
                Arg::with_name("start_slot")
                    .index(1)
                    .value_name("SLOT")
                    .takes_value(true)
                    .required(true)
                    .help("Start slot to purge from (inclusive)"),
            )
            .arg(Arg::with_name("end_slot").index(2).value_name("SLOT").help(
                "Ending slot to stop purging (inclusive) \
                [default: the highest slot in the ledger]",
            ))
            .arg(
                Arg::with_name("batch_size")
                    .long("batch-size")
                    .value_name("NUM")
                    .takes_value(true)
                    .default_value("1000")
                    .help("Removes at most BATCH_SIZE slots while purging in loop"),
            )
            .arg(
                Arg::with_name("no_compaction")
                    .long("no-compaction")
                    .required(false)
                    .takes_value(false)
                    .help(
                        "--no-compaction is deprecated, ledger compaction after purge is \
                         disabled by default",
                    )
                    .conflicts_with("enable_compaction")
                    .hidden(hidden_unless_forced()),
            )
            .arg(
                Arg::with_name("enable_compaction")
                    .long("enable-compaction")
                    .required(false)
                    .takes_value(false)
                    .help(
                        "Perform ledger compaction after purge. Compaction will optimize \
                         storage space, but may take a long time to complete.",
                    )
                    .conflicts_with("no_compaction"),
            )
            .arg(
                Arg::with_name("dead_slots_only")
                    .long("dead-slots-only")
                    .required(false)
                    .takes_value(false)
                    .help("Limit purging to dead slots only"),
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
        SubCommand::with_name("shred-meta")
            .about("Prints raw shred metadata")
            .settings(&hidden)
            .arg(&starting_slot_arg)
            .arg(&ending_slot_arg),
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
        ("copy", Some(arg_matches)) => {
            let starting_slot = value_t_or_exit!(arg_matches, "starting_slot", Slot);
            let ending_slot = value_t_or_exit!(arg_matches, "ending_slot", Slot);
            let target_db = PathBuf::from(value_t_or_exit!(arg_matches, "target_db", String));

            let source = crate::open_blockstore(&ledger_path, arg_matches, AccessType::Secondary);

            // Check if shred storage type can be inferred; if not, a new
            // ledger is being created. open_blockstore() will attempt to
            // to infer shred storage type as well, but this check provides
            // extra insight to user on how to create a FIFO ledger.
            let _ = get_shred_storage_type(
                &target_db,
                &format!(
                    "No --target-db ledger at {:?} was detected, default compaction \
                 (RocksLevel) will be used. Fifo compaction can be enabled for a new \
                 ledger by manually creating {BLOCKSTORE_DIRECTORY_ROCKS_FIFO} directory \
                 within the specified --target_db directory.",
                    &target_db
                ),
            );
            let target = crate::open_blockstore(&target_db, arg_matches, AccessType::Primary);
            for (slot, _meta) in source.slot_meta_iterator(starting_slot).unwrap() {
                if slot > ending_slot {
                    break;
                }
                if let Ok(shreds) = source.get_data_shreds_for_slot(slot, 0) {
                    if target.insert_shreds(shreds, None, true).is_err() {
                        warn!("error inserting shreds for slot {}", slot);
                    }
                }
            }
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
        ("purge", Some(arg_matches)) => {
            let start_slot = value_t_or_exit!(arg_matches, "start_slot", Slot);
            let end_slot = value_t!(arg_matches, "end_slot", Slot).ok();
            let perform_compaction = arg_matches.is_present("enable_compaction");
            if arg_matches.is_present("no_compaction") {
                warn!("--no-compaction is deprecated and is now the default behavior.");
            }
            let dead_slots_only = arg_matches.is_present("dead_slots_only");
            let batch_size = value_t_or_exit!(arg_matches, "batch_size", usize);

            let blockstore = crate::open_blockstore(
                &ledger_path,
                arg_matches,
                AccessType::PrimaryForMaintenance,
            );

            let end_slot = match end_slot {
                Some(end_slot) => end_slot,
                None => match blockstore.slot_meta_iterator(start_slot) {
                    Ok(metas) => {
                        let slots: Vec<_> = metas.map(|(slot, _)| slot).collect();
                        if slots.is_empty() {
                            eprintln!("Purge range is empty");
                            std::process::exit(1);
                        }
                        *slots.last().unwrap()
                    }
                    Err(err) => {
                        eprintln!("Unable to read the Ledger: {err:?}");
                        std::process::exit(1);
                    }
                },
            };

            if end_slot < start_slot {
                eprintln!("end slot {end_slot} is less than start slot {start_slot}");
                std::process::exit(1);
            }
            info!(
                "Purging data from slots {} to {} ({} slots) (do compaction: {}) \
                (dead slot only: {})",
                start_slot,
                end_slot,
                end_slot - start_slot,
                perform_compaction,
                dead_slots_only,
            );
            let purge_from_blockstore = |start_slot, end_slot| {
                blockstore.purge_from_next_slots(start_slot, end_slot);
                if perform_compaction {
                    blockstore.purge_and_compact_slots(start_slot, end_slot);
                } else {
                    blockstore.purge_slots(start_slot, end_slot, PurgeType::Exact);
                }
            };
            if !dead_slots_only {
                let slots_iter = &(start_slot..=end_slot).chunks(batch_size);
                for slots in slots_iter {
                    let slots = slots.collect::<Vec<_>>();
                    assert!(!slots.is_empty());

                    let start_slot = *slots.first().unwrap();
                    let end_slot = *slots.last().unwrap();
                    info!(
                        "Purging chunked slots from {} to {} ({} slots)",
                        start_slot,
                        end_slot,
                        end_slot - start_slot
                    );
                    purge_from_blockstore(start_slot, end_slot);
                }
            } else {
                let dead_slots_iter = blockstore
                    .dead_slots_iterator(start_slot)
                    .unwrap()
                    .take_while(|s| *s <= end_slot);
                for dead_slot in dead_slots_iter {
                    info!("Purging dead slot {}", dead_slot);
                    purge_from_blockstore(dead_slot, dead_slot);
                }
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
        ("shred-meta", Some(arg_matches)) => {
            #[derive(Debug)]
            #[allow(dead_code)]
            struct ShredMeta<'a> {
                slot: Slot,
                full_slot: bool,
                shred_index: usize,
                data: bool,
                code: bool,
                last_in_slot: bool,
                data_complete: bool,
                shred: &'a Shred,
            }
            let starting_slot = value_t_or_exit!(arg_matches, "starting_slot", Slot);
            let ending_slot = value_t!(arg_matches, "ending_slot", Slot).unwrap_or(Slot::MAX);
            let ledger = crate::open_blockstore(&ledger_path, arg_matches, AccessType::Secondary);
            for (slot, _meta) in ledger
                .slot_meta_iterator(starting_slot)
                .unwrap()
                .take_while(|(slot, _)| *slot <= ending_slot)
            {
                let full_slot = ledger.is_full(slot);
                if let Ok(shreds) = ledger.get_data_shreds_for_slot(slot, 0) {
                    for (shred_index, shred) in shreds.iter().enumerate() {
                        println!(
                            "{:#?}",
                            ShredMeta {
                                slot,
                                full_slot,
                                shred_index,
                                data: shred.is_data(),
                                code: shred.is_code(),
                                data_complete: shred.data_complete(),
                                last_in_slot: shred.last_in_slot(),
                                shred,
                            }
                        );
                    }
                }
            }
        }
        _ => unreachable!(),
    }
}
