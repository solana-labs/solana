//! The `blockstore` subcommand

use {
    crate::ledger_path::canonicalize_ledger_path,
    clap::{App, AppSettings, ArgMatches, SubCommand},
    serde_json::json,
    solana_ledger::{
        blockstore_db::{self, Database},
        blockstore_options::AccessType,
    },
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

    vec![SubCommand::with_name("analyze-storage")
        .about("Output statistics in JSON format about all column families in the ledger rocksdb")
        .settings(&hidden)]
}

pub fn blockstore_process_command(ledger_path: &Path, matches: &ArgMatches<'_>) {
    let ledger_path = canonicalize_ledger_path(ledger_path);

    match matches.subcommand() {
        ("analyze-storage", Some(arg_matches)) => {
            analyze_storage(
                &crate::open_blockstore(&ledger_path, arg_matches, AccessType::Secondary).db(),
            );
        }
        _ => unreachable!(),
    }
}
