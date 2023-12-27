//! The `blockstore` subcommand

use {
    crate::{
        ledger_path::canonicalize_ledger_path,
        output::{SlotBounds, SlotInfo},
    },
    clap::{value_t_or_exit, App, AppSettings, Arg, ArgMatches, SubCommand},
    serde_json::json,
    solana_cli_output::OutputFormat,
    solana_ledger::{
        blockstore_db::{self, Database},
        blockstore_options::AccessType,
    },
    solana_sdk::clock::Slot,
    std::path::Path,
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
        _ => unreachable!(),
    }
}
