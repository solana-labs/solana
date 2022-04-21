#![allow(clippy::integer_arithmetic)]
use {
    crate::{bigtable::*, ledger_path::*},
    clap::{
        crate_description, crate_name, value_t, value_t_or_exit, values_t_or_exit, App,
        AppSettings, Arg, ArgMatches, SubCommand,
    },
    dashmap::DashMap,
    itertools::Itertools,
    log::*,
    regex::Regex,
    serde::Serialize,
    serde_json::json,
    solana_clap_utils::{
        input_parsers::{cluster_type_of, pubkey_of, pubkeys_of},
        input_validators::{
            is_parsable, is_pow2, is_pubkey, is_pubkey_or_keypair, is_slot, is_valid_percentage,
        },
    },
    solana_core::system_monitor_service::SystemMonitorService,
    solana_entry::entry::Entry,
    solana_ledger::{
        ancestor_iterator::AncestorIterator,
        bank_forks_utils,
        blockstore::{create_new_ledger, Blockstore, PurgeType},
        blockstore_db::{
            self, AccessType, BlockstoreOptions, BlockstoreRecoveryMode, Database,
            LedgerColumnOptions,
        },
        blockstore_processor::{BlockstoreProcessorError, ProcessOptions},
        shred::Shred,
    },
    solana_measure::measure::Measure,
    solana_runtime::{
        accounts_db::{AccountsDbConfig, FillerAccountsConfig},
        accounts_index::{AccountsIndexConfig, IndexLimitMb, ScanConfig},
        bank::{Bank, RewardCalculationEvent},
        bank_forks::BankForks,
        cost_model::CostModel,
        cost_tracker::CostTracker,
        hardened_unpack::{open_genesis_config, MAX_GENESIS_ARCHIVE_UNPACKED_SIZE},
        runtime_config::RuntimeConfig,
        snapshot_archive_info::SnapshotArchiveInfoGetter,
        snapshot_config::SnapshotConfig,
        snapshot_hash::StartingSnapshotHashes,
        snapshot_utils::{
            self, ArchiveFormat, SnapshotVersion, DEFAULT_MAX_FULL_SNAPSHOT_ARCHIVES_TO_RETAIN,
            DEFAULT_MAX_INCREMENTAL_SNAPSHOT_ARCHIVES_TO_RETAIN,
        },
    },
    solana_sdk::{
        account::{AccountSharedData, ReadableAccount, WritableAccount},
        account_utils::StateMut,
        clock::{Epoch, Slot},
        feature::{self, Feature},
        feature_set,
        genesis_config::{ClusterType, GenesisConfig},
        hash::Hash,
        inflation::Inflation,
        native_token::{lamports_to_sol, sol_to_lamports, Sol},
        pubkey::Pubkey,
        rent::Rent,
        shred_version::compute_shred_version,
        stake::{self, state::StakeState},
        system_program,
        transaction::{MessageHash, SanitizedTransaction, SimpleAddressLoader},
    },
    solana_stake_program::stake_state::{self, PointValue},
    solana_transaction_status::VersionedTransactionWithStatusMeta,
    solana_vote_program::{
        self,
        vote_state::{self, VoteState},
    },
    std::{
        collections::{BTreeMap, BTreeSet, HashMap, HashSet},
        ffi::OsStr,
        fs::File,
        io::{self, stdout, BufRead, BufReader, Write},
        path::{Path, PathBuf},
        process::{exit, Command, Stdio},
        str::FromStr,
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc, RwLock,
        },
    },
};

mod bigtable;
mod ledger_path;

#[derive(PartialEq)]
enum LedgerOutputMethod {
    Print,
    Json,
}

fn output_slot_rewards(blockstore: &Blockstore, slot: Slot, method: &LedgerOutputMethod) {
    // Note: rewards are not output in JSON yet
    if *method == LedgerOutputMethod::Print {
        if let Ok(Some(rewards)) = blockstore.read_rewards(slot) {
            if !rewards.is_empty() {
                println!("  Rewards:");
                println!(
                    "    {:<44}  {:^15}  {:<15}  {:<20}  {:>10}",
                    "Address", "Type", "Amount", "New Balance", "Commission",
                );

                for reward in rewards {
                    let sign = if reward.lamports < 0 { "-" } else { "" };
                    println!(
                        "    {:<44}  {:^15}  {}◎{:<14.9}  ◎{:<18.9}   {}",
                        reward.pubkey,
                        if let Some(reward_type) = reward.reward_type {
                            format!("{}", reward_type)
                        } else {
                            "-".to_string()
                        },
                        sign,
                        lamports_to_sol(reward.lamports.unsigned_abs()),
                        lamports_to_sol(reward.post_balance),
                        reward
                            .commission
                            .map(|commission| format!("{:>9}%", commission))
                            .unwrap_or_else(|| "    -".to_string())
                    );
                }
            }
        }
    }
}

fn output_entry(
    blockstore: &Blockstore,
    method: &LedgerOutputMethod,
    slot: Slot,
    entry_index: usize,
    entry: Entry,
) {
    match method {
        LedgerOutputMethod::Print => {
            println!(
                "  Entry {} - num_hashes: {}, hash: {}, transactions: {}",
                entry_index,
                entry.num_hashes,
                entry.hash,
                entry.transactions.len()
            );
            for (transactions_index, transaction) in entry.transactions.into_iter().enumerate() {
                println!("    Transaction {}", transactions_index);
                let tx_signature = transaction.signatures[0];
                let tx_with_meta = blockstore
                    .read_transaction_status((tx_signature, slot))
                    .unwrap_or_else(|err| {
                        eprintln!(
                            "Failed to read transaction status for {} at slot {}: {}",
                            transaction.signatures[0], slot, err
                        );
                        None
                    })
                    .map(|meta| VersionedTransactionWithStatusMeta { transaction, meta });

                if let Some(tx_with_meta) = tx_with_meta {
                    let status = tx_with_meta.meta.into();
                    solana_cli_output::display::println_transaction(
                        &tx_with_meta.transaction,
                        Some(&status),
                        "      ",
                        None,
                        None,
                    );
                }
            }
        }
        LedgerOutputMethod::Json => {
            // Note: transaction status is not output in JSON yet
            serde_json::to_writer(stdout(), &entry).expect("serialize entry");
            stdout().write_all(b",\n").expect("newline");
        }
    }
}

fn output_slot(
    blockstore: &Blockstore,
    slot: Slot,
    allow_dead_slots: bool,
    method: &LedgerOutputMethod,
    verbose_level: u64,
) -> Result<(), String> {
    if blockstore.is_dead(slot) {
        if allow_dead_slots {
            if *method == LedgerOutputMethod::Print {
                println!(" Slot is dead");
            }
        } else {
            return Err("Dead slot".to_string());
        }
    }

    let (entries, num_shreds, is_full) = blockstore
        .get_slot_entries_with_shred_info(slot, 0, allow_dead_slots)
        .map_err(|err| format!("Failed to load entries for slot {}: {:?}", slot, err))?;

    if *method == LedgerOutputMethod::Print {
        if let Ok(Some(meta)) = blockstore.meta(slot) {
            if verbose_level >= 2 {
                println!(" Slot Meta {:?} is_full: {}", meta, is_full);
            } else {
                println!(
                    " num_shreds: {}, parent_slot: {:?}, num_entries: {}, is_full: {}",
                    num_shreds,
                    meta.parent_slot,
                    entries.len(),
                    is_full,
                );
            }
        }
    }

    if verbose_level >= 2 {
        for (entry_index, entry) in entries.into_iter().enumerate() {
            output_entry(blockstore, method, slot, entry_index, entry);
        }

        output_slot_rewards(blockstore, slot, method);
    } else if verbose_level >= 1 {
        let mut transactions = 0;
        let mut num_hashes = 0;
        let mut program_ids = HashMap::new();
        let blockhash = if let Some(entry) = entries.last() {
            entry.hash
        } else {
            Hash::default()
        };

        for entry in entries {
            transactions += entry.transactions.len();
            num_hashes += entry.num_hashes;
            for transaction in entry.transactions {
                let tx_signature = transaction.signatures[0];
                let sanitize_result = SanitizedTransaction::try_create(
                    transaction,
                    MessageHash::Compute,
                    None,
                    SimpleAddressLoader::Disabled,
                    true, // require_static_program_ids
                );

                match sanitize_result {
                    Ok(transaction) => {
                        for (program_id, _) in transaction.message().program_instructions_iter() {
                            *program_ids.entry(*program_id).or_insert(0) += 1;
                        }
                    }
                    Err(err) => {
                        warn!(
                            "Failed to analyze unsupported transaction {}: {:?}",
                            tx_signature, err
                        );
                    }
                }
            }
        }

        println!(
            "  Transactions: {}, hashes: {}, block_hash: {}",
            transactions, num_hashes, blockhash,
        );
        println!("  Programs: {:?}", program_ids);
    }
    Ok(())
}

fn output_ledger(
    blockstore: Blockstore,
    starting_slot: Slot,
    ending_slot: Slot,
    allow_dead_slots: bool,
    method: LedgerOutputMethod,
    num_slots: Option<Slot>,
    verbose_level: u64,
    only_rooted: bool,
) {
    let slot_iterator = blockstore
        .slot_meta_iterator(starting_slot)
        .unwrap_or_else(|err| {
            eprintln!(
                "Failed to load entries starting from slot {}: {:?}",
                starting_slot, err
            );
            exit(1);
        });

    if method == LedgerOutputMethod::Json {
        stdout().write_all(b"{\"ledger\":[\n").expect("open array");
    }

    let num_slots = num_slots.unwrap_or(Slot::MAX);
    let mut num_printed = 0;
    for (slot, slot_meta) in slot_iterator {
        if only_rooted && !blockstore.is_root(slot) {
            continue;
        }
        if slot > ending_slot {
            break;
        }

        match method {
            LedgerOutputMethod::Print => {
                println!("Slot {} root?: {}", slot, blockstore.is_root(slot))
            }
            LedgerOutputMethod::Json => {
                serde_json::to_writer(stdout(), &slot_meta).expect("serialize slot_meta");
                stdout().write_all(b",\n").expect("newline");
            }
        }

        if let Err(err) = output_slot(&blockstore, slot, allow_dead_slots, &method, verbose_level) {
            eprintln!("{}", err);
        }
        num_printed += 1;
        if num_printed >= num_slots as usize {
            break;
        }
    }

    if method == LedgerOutputMethod::Json {
        stdout().write_all(b"\n]}\n").expect("close array");
    }
}

fn output_account(
    pubkey: &Pubkey,
    account: &AccountSharedData,
    modified_slot: Option<Slot>,
    print_account_data: bool,
) {
    println!("{}", pubkey);
    println!("  balance: {} SOL", lamports_to_sol(account.lamports()));
    println!("  owner: '{}'", account.owner());
    println!("  executable: {}", account.executable());
    if let Some(slot) = modified_slot {
        println!("  slot: {}", slot);
    }
    println!("  rent_epoch: {}", account.rent_epoch());
    println!("  data_len: {}", account.data().len());
    if print_account_data {
        println!("  data: '{}'", bs58::encode(account.data()).into_string());
    }
}

fn render_dot(dot: String, output_file: &str, output_format: &str) -> io::Result<()> {
    let mut child = Command::new("dot")
        .arg(format!("-T{}", output_format))
        .arg(format!("-o{}", output_file))
        .stdin(Stdio::piped())
        .spawn()
        .map_err(|err| {
            eprintln!("Failed to spawn dot: {:?}", err);
            err
        })?;

    let stdin = child.stdin.as_mut().unwrap();
    stdin.write_all(&dot.into_bytes())?;

    let status = child.wait_with_output()?.status;
    if !status.success() {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            format!("dot failed with error {}", status.code().unwrap_or(-1)),
        ));
    }
    Ok(())
}

#[allow(clippy::cognitive_complexity)]
fn graph_forks(bank_forks: &BankForks, include_all_votes: bool) -> String {
    let frozen_banks = bank_forks.frozen_banks();
    let mut fork_slots: HashSet<_> = frozen_banks.keys().cloned().collect();
    for (_, bank) in frozen_banks {
        for parent in bank.parents() {
            fork_slots.remove(&parent.slot());
        }
    }

    // Search all forks and collect the last vote made by each validator
    let mut last_votes = HashMap::new();
    let default_vote_state = VoteState::default();
    for fork_slot in &fork_slots {
        let bank = &bank_forks[*fork_slot];

        let total_stake = bank
            .vote_accounts()
            .iter()
            .map(|(_, (stake, _))| stake)
            .sum();
        for (stake, vote_account) in bank.vote_accounts().values() {
            let vote_state = vote_account.vote_state();
            let vote_state = vote_state.as_ref().unwrap_or(&default_vote_state);
            if let Some(last_vote) = vote_state.votes.iter().last() {
                let entry = last_votes.entry(vote_state.node_pubkey).or_insert((
                    last_vote.slot,
                    vote_state.clone(),
                    *stake,
                    total_stake,
                ));
                if entry.0 < last_vote.slot {
                    *entry = (last_vote.slot, vote_state.clone(), *stake, total_stake);
                }
            }
        }
    }

    // Figure the stake distribution at all the nodes containing the last vote from each
    // validator
    let mut slot_stake_and_vote_count = HashMap::new();
    for (last_vote_slot, _, stake, total_stake) in last_votes.values() {
        let entry = slot_stake_and_vote_count
            .entry(last_vote_slot)
            .or_insert((0, 0, *total_stake));
        entry.0 += 1;
        entry.1 += stake;
        assert_eq!(entry.2, *total_stake)
    }

    let mut dot = vec!["digraph {".to_string()];

    // Build a subgraph consisting of all banks and links to their parent banks
    dot.push("  subgraph cluster_banks {".to_string());
    dot.push("    style=invis".to_string());
    let mut styled_slots = HashSet::new();
    let mut all_votes: HashMap<Pubkey, HashMap<Slot, VoteState>> = HashMap::new();
    for fork_slot in &fork_slots {
        let mut bank = bank_forks[*fork_slot].clone();

        let mut first = true;
        loop {
            for (_, vote_account) in bank.vote_accounts().values() {
                let vote_state = vote_account.vote_state();
                let vote_state = vote_state.as_ref().unwrap_or(&default_vote_state);
                if let Some(last_vote) = vote_state.votes.iter().last() {
                    let validator_votes = all_votes.entry(vote_state.node_pubkey).or_default();
                    validator_votes
                        .entry(last_vote.slot)
                        .or_insert_with(|| vote_state.clone());
                }
            }

            if !styled_slots.contains(&bank.slot()) {
                dot.push(format!(
                    r#"    "{}"[label="{} (epoch {})\nleader: {}{}{}",style="{}{}"];"#,
                    bank.slot(),
                    bank.slot(),
                    bank.epoch(),
                    bank.collector_id(),
                    if let Some(parent) = bank.parent() {
                        format!(
                            "\ntransactions: {}",
                            bank.transaction_count() - parent.transaction_count(),
                        )
                    } else {
                        "".to_string()
                    },
                    if let Some((votes, stake, total_stake)) =
                        slot_stake_and_vote_count.get(&bank.slot())
                    {
                        format!(
                            "\nvotes: {}, stake: {:.1} SOL ({:.1}%)",
                            votes,
                            lamports_to_sol(*stake),
                            *stake as f64 / *total_stake as f64 * 100.,
                        )
                    } else {
                        "".to_string()
                    },
                    if first { "filled," } else { "" },
                    ""
                ));
                styled_slots.insert(bank.slot());
            }
            first = false;

            match bank.parent() {
                None => {
                    if bank.slot() > 0 {
                        dot.push(format!(r#"    "{}" -> "..." [dir=back]"#, bank.slot(),));
                    }
                    break;
                }
                Some(parent) => {
                    let slot_distance = bank.slot() - parent.slot();
                    let penwidth = if bank.epoch() > parent.epoch() {
                        "5"
                    } else {
                        "1"
                    };
                    let link_label = if slot_distance > 1 {
                        format!("label=\"{} slots\",color=red", slot_distance)
                    } else {
                        "color=blue".to_string()
                    };
                    dot.push(format!(
                        r#"    "{}" -> "{}"[{},dir=back,penwidth={}];"#,
                        bank.slot(),
                        parent.slot(),
                        link_label,
                        penwidth
                    ));

                    bank = parent.clone();
                }
            }
        }
    }
    dot.push("  }".to_string());

    // Strafe the banks with links from validators to the bank they last voted on,
    // while collecting information about the absent votes and stakes
    let mut absent_stake = 0;
    let mut absent_votes = 0;
    let mut lowest_last_vote_slot = std::u64::MAX;
    let mut lowest_total_stake = 0;
    for (node_pubkey, (last_vote_slot, vote_state, stake, total_stake)) in &last_votes {
        all_votes.entry(*node_pubkey).and_modify(|validator_votes| {
            validator_votes.remove(last_vote_slot);
        });

        dot.push(format!(
            r#"  "last vote {}"[shape=box,label="Latest validator vote: {}\nstake: {} SOL\nroot slot: {}\nvote history:\n{}"];"#,
            node_pubkey,
            node_pubkey,
            lamports_to_sol(*stake),
            vote_state.root_slot.unwrap_or(0),
            vote_state
                .votes
                .iter()
                .map(|vote| format!("slot {} (conf={})", vote.slot, vote.confirmation_count))
                .collect::<Vec<_>>()
                .join("\n")
        ));

        dot.push(format!(
            r#"  "last vote {}" -> "{}" [style=dashed,label="latest vote"];"#,
            node_pubkey,
            if styled_slots.contains(last_vote_slot) {
                last_vote_slot.to_string()
            } else {
                if *last_vote_slot < lowest_last_vote_slot {
                    lowest_last_vote_slot = *last_vote_slot;
                    lowest_total_stake = *total_stake;
                }
                absent_votes += 1;
                absent_stake += stake;

                "...".to_string()
            },
        ));
    }

    // Annotate the final "..." node with absent vote and stake information
    if absent_votes > 0 {
        dot.push(format!(
            r#"    "..."[label="...\nvotes: {}, stake: {:.1} SOL {:.1}%"];"#,
            absent_votes,
            lamports_to_sol(absent_stake),
            absent_stake as f64 / lowest_total_stake as f64 * 100.,
        ));
    }

    // Add for vote information from all banks.
    if include_all_votes {
        for (node_pubkey, validator_votes) in &all_votes {
            for (vote_slot, vote_state) in validator_votes {
                dot.push(format!(
                    r#"  "{} vote {}"[shape=box,style=dotted,label="validator vote: {}\nroot slot: {}\nvote history:\n{}"];"#,
                    node_pubkey,
                    vote_slot,
                    node_pubkey,
                    vote_state.root_slot.unwrap_or(0),
                    vote_state
                        .votes
                        .iter()
                        .map(|vote| format!("slot {} (conf={})", vote.slot, vote.confirmation_count))
                        .collect::<Vec<_>>()
                        .join("\n")
                ));

                dot.push(format!(
                    r#"  "{} vote {}" -> "{}" [style=dotted,label="vote"];"#,
                    node_pubkey,
                    vote_slot,
                    if styled_slots.contains(vote_slot) {
                        vote_slot.to_string()
                    } else {
                        "...".to_string()
                    },
                ));
            }
        }
    }

    dot.push("}".to_string());
    dot.join("\n")
}

fn analyze_column<
    C: solana_ledger::blockstore_db::Column + solana_ledger::blockstore_db::ColumnName,
>(
    db: &Database,
    name: &str,
) {
    let mut key_tot: u64 = 0;
    let mut val_hist = histogram::Histogram::new();
    let mut val_tot: u64 = 0;
    let mut row_hist = histogram::Histogram::new();
    let a = C::key_size() as u64;
    for (_x, y) in db.iter::<C>(blockstore_db::IteratorMode::Start).unwrap() {
        let b = y.len() as u64;
        key_tot += a;
        val_hist.increment(b).unwrap();
        val_tot += b;
        row_hist.increment(a + b).unwrap();
    }

    let json_result = if val_hist.entries() > 0 {
        json!({
            "column":name,
            "entries":val_hist.entries(),
            "key_stats":{
                "max":a,
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
            "max":a,
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
    use blockstore_db::columns::*;
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
}

fn open_blockstore(
    ledger_path: &Path,
    access_type: AccessType,
    wal_recovery_mode: Option<BlockstoreRecoveryMode>,
) -> Blockstore {
    match Blockstore::open_with_options(
        ledger_path,
        BlockstoreOptions {
            access_type,
            recovery_mode: wal_recovery_mode,
            enforce_ulimit_nofile: true,
            ..BlockstoreOptions::default()
        },
    ) {
        Ok(blockstore) => blockstore,
        Err(err) => {
            eprintln!("Failed to open ledger at {:?}: {:?}", ledger_path, err);
            exit(1);
        }
    }
}

// This function is duplicated in validator/src/main.rs...
fn hardforks_of(matches: &ArgMatches<'_>, name: &str) -> Option<Vec<Slot>> {
    if matches.is_present(name) {
        Some(values_t_or_exit!(matches, name, Slot))
    } else {
        None
    }
}

fn load_bank_forks(
    arg_matches: &ArgMatches,
    genesis_config: &GenesisConfig,
    blockstore: &Blockstore,
    process_options: ProcessOptions,
    snapshot_archive_path: Option<PathBuf>,
) -> Result<(Arc<RwLock<BankForks>>, Option<StartingSnapshotHashes>), BlockstoreProcessorError> {
    let bank_snapshots_dir = blockstore
        .ledger_path()
        .join(if blockstore.is_primary_access() {
            "snapshot"
        } else {
            "snapshot.ledger-tool"
        });
    let snapshot_config = if arg_matches.is_present("no_snapshot") {
        None
    } else {
        let snapshot_archives_dir =
            snapshot_archive_path.unwrap_or_else(|| blockstore.ledger_path().to_path_buf());
        Some(SnapshotConfig {
            full_snapshot_archive_interval_slots: Slot::MAX,
            incremental_snapshot_archive_interval_slots: Slot::MAX,
            snapshot_archives_dir,
            bank_snapshots_dir,
            ..SnapshotConfig::default()
        })
    };
    let account_paths = if let Some(account_paths) = arg_matches.value_of("account_paths") {
        if !blockstore.is_primary_access() {
            // Be defensive, when default account dir is explicitly specified, it's still possible
            // to wipe the dir possibly shared by the running validator!
            eprintln!("Error: custom accounts path is not supported under secondary access");
            exit(1);
        }
        account_paths.split(',').map(PathBuf::from).collect()
    } else if blockstore.is_primary_access() {
        vec![blockstore.ledger_path().join("accounts")]
    } else {
        let non_primary_accounts_path = blockstore.ledger_path().join("accounts.ledger-tool");
        info!(
            "Default accounts path is switched aligning with Blockstore's secondary access: {:?}",
            non_primary_accounts_path
        );

        if non_primary_accounts_path.exists() {
            info!("Clearing {:?}", non_primary_accounts_path);
            if let Err(err) = std::fs::remove_dir_all(&non_primary_accounts_path) {
                eprintln!(
                    "error deleting accounts path {:?}: {}",
                    non_primary_accounts_path, err
                );
                exit(1);
            }
        }

        vec![non_primary_accounts_path]
    };

    bank_forks_utils::load(
        genesis_config,
        blockstore,
        account_paths,
        None,
        snapshot_config.as_ref(),
        process_options,
        None,
        None,
        None,
    )
    .map(|(bank_forks, .., starting_snapshot_hashes)| (bank_forks, starting_snapshot_hashes))
}

fn compute_slot_cost(blockstore: &Blockstore, slot: Slot) -> Result<(), String> {
    if blockstore.is_dead(slot) {
        return Err("Dead slot".to_string());
    }

    let (entries, _num_shreds, _is_full) = blockstore
        .get_slot_entries_with_shred_info(slot, 0, false)
        .map_err(|err| format!(" Slot: {}, Failed to load entries, err {:?}", slot, err))?;

    let num_entries = entries.len();
    let mut num_transactions = 0;
    let mut num_programs = 0;

    let mut program_ids = HashMap::new();
    let mut cost_model = CostModel::default();
    cost_model.initialize_cost_table(&blockstore.read_program_costs().unwrap());
    let mut cost_tracker = CostTracker::default();

    for entry in entries {
        num_transactions += entry.transactions.len();
        entry
            .transactions
            .into_iter()
            .filter_map(|transaction| {
                SanitizedTransaction::try_create(
                    transaction,
                    MessageHash::Compute,
                    None,
                    SimpleAddressLoader::Disabled,
                    true, // require_static_program_ids
                )
                .map_err(|err| {
                    warn!("Failed to compute cost of transaction: {:?}", err);
                })
                .ok()
            })
            .for_each(|transaction| {
                num_programs += transaction.message().instructions().len();

                let tx_cost = cost_model.calculate_cost(&transaction);
                let result = cost_tracker.try_add(&tx_cost);
                if result.is_err() {
                    println!(
                        "Slot: {}, CostModel rejected transaction {:?}, reason {:?}",
                        slot, transaction, result,
                    );
                }
                for (program_id, _instruction) in transaction.message().program_instructions_iter()
                {
                    *program_ids.entry(*program_id).or_insert(0) += 1;
                }
            });
    }

    println!(
        "Slot: {}, Entries: {}, Transactions: {}, Programs {}",
        slot, num_entries, num_transactions, num_programs,
    );
    println!("  Programs: {:?}", program_ids);

    Ok(())
}

fn open_genesis_config_by(ledger_path: &Path, matches: &ArgMatches<'_>) -> GenesisConfig {
    let max_genesis_archive_unpacked_size =
        value_t_or_exit!(matches, "max_genesis_archive_unpacked_size", u64);
    open_genesis_config(ledger_path, max_genesis_archive_unpacked_size)
}

fn assert_capitalization(bank: &Bank) {
    let debug_verify = true;
    assert!(bank.calculate_and_verify_capitalization(debug_verify));
}
#[cfg(not(target_env = "msvc"))]
use jemallocator::Jemalloc;

#[cfg(not(target_env = "msvc"))]
#[global_allocator]
static GLOBAL: Jemalloc = Jemalloc;

#[allow(clippy::cognitive_complexity)]
fn main() {
    // Ignore SIGUSR1 to prevent long-running calls being killed by logrotate
    // in warehouse deployments
    #[cfg(unix)]
    {
        // `register()` is unsafe because the action is called in a signal handler
        // with the usual caveats. So long as this action body stays empty, we'll
        // be fine
        unsafe { signal_hook::low_level::register(signal_hook::consts::SIGUSR1, || {}) }.unwrap();
    }

    const DEFAULT_ROOT_COUNT: &str = "1";
    const DEFAULT_MAX_SLOTS_ROOT_REPAIR: &str = "2000";
    solana_logger::setup_with_default("solana=info");

    let interesting = 
    [
    Pubkey::from_str("5PjLaJCTHRzLoAfKY2fvHMqcABHV4vGbnyJ8qoAtakc6").unwrap(),
    Pubkey::from_str("5PjMZVHXNJM7ewASiXLrQUTkNzCk6DQEZoooaKP1Zreq").unwrap(),
    Pubkey::from_str("GRi5d3jn7isiL44kwrrqCmHsjuajtzugie35FF51ghmd").unwrap(),
    Pubkey::from_str("5PjJqVoGCDrAGchEM24xJX2fu53dfh7qQxbNCTFLyMTi").unwrap(),
    Pubkey::from_str("5PjGB9Zkx49hmFWSczGaLu7AJHTFCksT7eUNa3ykpVsE").unwrap(),
    Pubkey::from_str("5PjJjUfSgv37grxiXHpwFvYRhPxVqLpnmxGP29sGpuNm").unwrap(),
    Pubkey::from_str("5PjGc5ih6oCLmaq3PbQriEYdBni3jdH4XPgF4fn4xjRp").unwrap(),
    Pubkey::from_str("BByybB14PtpKy1n7SCVrXXKXYYCnahLrdxHKsF8gigPJ").unwrap(),
    Pubkey::from_str("xCcPEqqcAKr6zuqmha4rksv9oMb18jrhd3d93P7vFUo").unwrap(),
    Pubkey::from_str("5PjKoYrRXzuWB1T9FVz5LZsqmjPBmTiJU5rBY2Y3VBrf").unwrap(),
    Pubkey::from_str("5PjKvvF7epCYq9zWNq6TtmdgHhiDHpeNDEcQFsR8wsjv").unwrap(),
    Pubkey::from_str("5PjHMpmDr3m96J5VzLKkcDTCHnKscw61vyzAzGiTWx7F").unwrap(),
    Pubkey::from_str("5PjPac5kxe3PWbuTXsmEnrLg4Kr9cpSwh36J7HoUbofb").unwrap(),
    Pubkey::from_str("5PjKMzVXJ3rVXiByGrdhjr2xRfiVGcYcJ9yJ49ne1LSC").unwrap(),
    Pubkey::from_str("5PjPJRSGnB5P9xprYtLqZ78uymnPS9V6WJq96GgUHgqo").unwrap(),
    Pubkey::from_str("5PjLsSec3JGrhxtzQ8f4GQZL6VoTxENyyqbDsr1u4daK").unwrap(),
    Pubkey::from_str("ES1nh3hdKbMbZryDmok7bMWne8ZFygee1FxuybYExecT").unwrap(),
    Pubkey::from_str("5PjL1nTexaNAFUpNU6N44agtSTvZfs4hUmPJLLdHfkq6").unwrap(),
    Pubkey::from_str("5PjMNLT3WyGfq36F1MdUfkefhviysW9kCLEdypxAA7WQ").unwrap(),
    Pubkey::from_str("5PjMKaTK7eeM4nmzL2DDcy6rMpgty5ycs6kNjU4KCMph").unwrap(),
    Pubkey::from_str("5PjL31C3E9WyfwzDHHnSC27jvUUSVLqAFoqKNbqpWqWm").unwrap(),
    Pubkey::from_str("5PjM9w7eskBuzjRUS9dhdp8tBHjKjDEnDdSwhba1aDMM").unwrap(),
    Pubkey::from_str("5PjJHcgD1VhBrxw4bLMhe9EyrGcokrn6GG9TcSnD5v69").unwrap(),
    Pubkey::from_str("5PjJzcG7EKtkHhPxD7aFDSJpHLARZL4m2gcNDqJVRcbW").unwrap(),
    Pubkey::from_str("5PjPSQpFQS8xiwxpQomdGXzCGi51Y6d9ottg65KnSKS3").unwrap(),
    Pubkey::from_str("5PjK991Ae1xEeipXvYgseAXMEm74kVn3H7RTuWXF5JJX").unwrap(),
    Pubkey::from_str("5PjKFNJmsPpezoSfVbVZbYwRa1i2F1PsQ7VAu3BXR3hr").unwrap(),
    Pubkey::from_str("5PjN97aoApJmq6agkDnwFEHfkXxdvZfAv4BcegwrqJCL").unwrap(),
    Pubkey::from_str("5PjJF6E46BrMoLiNfQd55xKjPAhTMQFEEUA1dzkQ72U2").unwrap(),
    Pubkey::from_str("5PjH7A4E8U8xTwazR8KJAKpRipr36ZPYJaYVPW1VQGdD").unwrap(),
    Pubkey::from_str("5PjNvrWTQJf2vGGrHwNjSYEk5K1YwawhUHELZWhwQRFa").unwrap(),
    Pubkey::from_str("5PjHiSjn6YfqPJDhd3rox3Uo21jdMFAgt53z6cfDExTe").unwrap(),
    Pubkey::from_str("5PjNZzg5q6oFF2k3AwnUYPDhEdM2ayBnvHMPWqqcaZD5").unwrap(),
    Pubkey::from_str("5PjNogCRjMYxPghCkZuMXkj8118Hiy8npafmi8obh79d").unwrap(),
    Pubkey::from_str("5PjLoDAXbwtA6RcMzq1ZBPNeqbWmLzaAouAKk9EqMFr3").unwrap(),
    Pubkey::from_str("5PjGuCNouD3AFgXhAdQxSf7s8igAC6XqktuD5UkZiyDK").unwrap(),
    Pubkey::from_str("5PjGs7WLavK5X213ELviTk6WV3XohTTntsQU9MgpocVd").unwrap(),
    Pubkey::from_str("5PjLpL6qZW9La6gibQCpprfea93bwyXqh5R9W7oQZP22").unwrap(),
    Pubkey::from_str("5PjMQgMgyrCmQi6jth9ZCRMGsgHzkyv7mw4SipWRiR9D").unwrap(),
    Pubkey::from_str("5PjLtN6i8for74Ag8FeX6BrtvDH5NDodPRJd8y67eWuR").unwrap(),
    Pubkey::from_str("5PjMQ48nALtxkQjCaDJwrDPydpXd2y9xUncWe8g3RZMj").unwrap(),
    Pubkey::from_str("5PjLsydEiUBSsoEwxw2hFnCBaPWnF8uWmjEy9JbE3BTj").unwrap(),
    Pubkey::from_str("5PjNdpQ5pBiX1F4EvruenxDu3RyX31o17CyHYtecd9Kd").unwrap(),
    Pubkey::from_str("5PjGnTFoutJggTbvH15k4D16JSgZ7QZwap7CvEiydFcG").unwrap(),
    Pubkey::from_str("5PjGK6Lp1BFSfNKFNu8juMN3d8Uu2DZPYRWMmGMRK8C9").unwrap(),
    Pubkey::from_str("5PjJNVjbFZce1BqiYJJwvf1ti1mqMwzzb6vt5QNhWcJJ").unwrap(),
    Pubkey::from_str("5PjM8osCU2irSUmTrNiEwnjqywnDF5cbdJkbTXVtw52u").unwrap(),
    Pubkey::from_str("5PjPnQM2vk3VWXmVPhVagWEsyvzU2CLLhEcs2fa8hbMc").unwrap(),
    Pubkey::from_str("5PjPioH9ETxZG3UUM6tgM94EmhEuhFk8y4AeCi6jDmja").unwrap(),
    Pubkey::from_str("9y3QYM5mcaB8tU7oXRzAQnzHVa75P8riDuPievLp64cY").unwrap(),
    Pubkey::from_str("B16JMAgpR84Dr6rucq4GYLZV7pdk1uPF533P9KVwNUq4").unwrap(),
    Pubkey::from_str("GXp68Kbc9xS2KnDYPJD6kVaAbsZP3xCaHfeXc6hWreJo").unwrap(),
    Pubkey::from_str("5PjHxcjRfA5KDA8Heuuq3rahQXPwK3qpJ9fFjU7LtQUm").unwrap(),
    Pubkey::from_str("5PjMgYnuX9U2wcbSoPE5WUyjQ8RgBA98TvdaP5MwRMe1").unwrap(),
    Pubkey::from_str("5PjL2ut3tjxfnPNdYE1yNZcsY9jhnBU9peFb4az2SLAd").unwrap(),
    Pubkey::from_str("5PjJXpn1gy9KDMZEXFZpYeoi1VJ3djgSZf3z27hnXtCF").unwrap(),
    Pubkey::from_str("5PjKNrcRLkrSn4sB5QNpTkv3GfQaQuoqSihxB5T6vLYL").unwrap(),
    Pubkey::from_str("5PjNFoXKX47T1Ai98Y3hF2jxCzUfbjMdX14GRcTEDzme").unwrap(),
    Pubkey::from_str("5PjJ3npf3v8vTo6nQ2FCF2Rbw74P3pxBTs8bciLzDo61").unwrap(),
    Pubkey::from_str("5PjGN9PspjeuUtHphpdwPkT377EvcRqXH6t9Kci7bku3").unwrap(),
    Pubkey::from_str("5PjHpDWALMa5QqUNeMi5J24PKuSGTncRQkWNeMjnj6Zi").unwrap(),
    Pubkey::from_str("5PjGpVYdiojcMyTucQBk9prD2VBq7GJMcsMegXNzKvN2").unwrap(),
    Pubkey::from_str("5PjNrZZbQX7Jf3ovMif7YmsdDnmcApUqZzwh5ocQwBCv").unwrap(),
    Pubkey::from_str("5PjJ5n77jRJc6izAGS65fXY47pJuqkWzGxCkAPhqoVU1").unwrap(),
    Pubkey::from_str("5PjNjZBJ7mWbu6FhLNyWeLTwCNchSwP3gQhGPrjkpWru").unwrap(),
    Pubkey::from_str("5PjHPpnryGSWbLajknAz2QFN1Dz5akixtPDrjfGAhef5").unwrap(),
    Pubkey::from_str("5PjHZiUnMPbqTcpweFCJNci86mDPDxRNMgZTFEpBNaqG").unwrap(),
    Pubkey::from_str("5PjMnnKjjTZjux8QcqtmuCAt7r6pEL6DKEWB3P99E39a").unwrap(),
    Pubkey::from_str("5PjHbuqZz3taQhpL4EyBiX1zJ3h1efKbFPLWtFyi2nW1").unwrap(),
    Pubkey::from_str("5PjP3esgJGuhcR3xSHjG5RNqJrYCqk7HnRGRrYmukNvS").unwrap(),
    Pubkey::from_str("5PjNZz3CowBB6K7wJvHjLaADhpuqFbZtfpRYWpQjjeDD").unwrap(),
    Pubkey::from_str("5PjKqqeH4xLRoKJn21S5uttZqnhE6uoL24nEvDLGB9y9").unwrap(),
    Pubkey::from_str("5PjPKAau76r3MSbgNDp1UpBeh2zp8vDwxVnvNzh7kFEp").unwrap(),
    Pubkey::from_str("5PjLnodYAzAKGuv4jis74HEhXuGkug5uppXGdUEhLS2Y").unwrap(),
    Pubkey::from_str("5PjPnmeqVFDsCkJ2ZaYxF4a4A4LapUrahk36rPTiy8fg").unwrap(),
    Pubkey::from_str("8tyCKXSjk8cm9bvNVhwEj62HizKPCBYNHBp4Z3hVRkFd").unwrap(),
    Pubkey::from_str("DojkWxTMc3NQkeKQ7Tg8c5uf5eZXBiQWxkyVahGQHtcZ").unwrap(),
    Pubkey::from_str("5PjJN6ec9c718JuFYkeTQE3DAR5aTAEoaJHPsQP6diZe").unwrap(),
    Pubkey::from_str("5PjP95AUFuYHtcAyMQCSw6j5WndV9FwtQy8gQ7qwkkaR").unwrap(),
    Pubkey::from_str("5PjNgmZCtehJQt95bCgSqnCShv8hQvUcsiNEMdDYm1Ne").unwrap(),
    Pubkey::from_str("5PjHmPWhht1mMeksgdeSow2yxAMW8hstkoYpUWSngD7q").unwrap(),
    Pubkey::from_str("5PjGDpgtjWAky1HC62AFX2kfPr55dCkWsMzsJANTNSgz").unwrap(),
    Pubkey::from_str("5PjHy3BBHZzPnv5LGhtfPWkyvYDkcNi5tCdWhpazMKk9").unwrap(),
    Pubkey::from_str("AzZrnBurWzCNLHBKNfgXMFgmcjvm3RDWU75qaoFKLX9z").unwrap(),
    Pubkey::from_str("JBMtUoKUSDTLHffvJmPg4pF36KjUaa5q6xEjkHsnBHxs").unwrap(),
    Pubkey::from_str("5PjGoWi1GnpiAZwCwtjRMG5CB8Lo2PyQYB6a98z29K4T").unwrap(),
    Pubkey::from_str("5PjK3CasjAepZ9ZQoo4RE6uspTNNymLd38jc1kQXWLn6").unwrap(),
    Pubkey::from_str("5PjKPF1kVBpRePQsTpa8Hs7jNDgg4AzEyd8tc9XnM7Bt").unwrap(),
    Pubkey::from_str("5PjKZ68aQkBRN8uFVssGqh8HhFkxeDDANt61P56dA8HN").unwrap(),
    Pubkey::from_str("5PjK57ENvHCkWQGdUtvUvMSCz7Sx13nGRQwfdo5zBts9").unwrap(),
    Pubkey::from_str("5PjJyH8kQ5UgVMYpFf6fhLG1j7CnrnVC6Z9m1PVLd6GF").unwrap(),
    Pubkey::from_str("8kzLcBCEC2xSXvGGG8jHjkBciqw3bPR1VwA1pCCPCpc9").unwrap(),
    Pubkey::from_str("5PjJ9phfBYnYmPpPWhGfu9Chpygm62pZocJEDa3siBYS").unwrap(),
    Pubkey::from_str("5PjKYD8RZ9pPT7PMfgYZPLBviMHj32jGUqMeJ4yHvkgZ").unwrap(),
    Pubkey::from_str("5PjP1rNMbjCsGSxnwYdVdkGkgM7yDK43CHud1PQ5zn3e").unwrap(),
    Pubkey::from_str("5PjKGF6kCGDzhqn5yJRcSttjQ3hULugjXecLSvcXYCfz").unwrap(),
    Pubkey::from_str("5PjMkwtQjw9LE5wqMjYo1TRsF4eZmC6oHYcrBzJ2qmQe").unwrap(),
    Pubkey::from_str("5PjPUfrMJhD6dAsMAjwJZkSfWjV7KXWr4PPuhy6jpzgX").unwrap(),
    Pubkey::from_str("5PjM1JipSrnwqAho635GYq9YaeJ2boNM3Sb5i5gJjoV9").unwrap(),
    Pubkey::from_str("5PjGW4cRa9jae6My4bUsxKfZ4hR7ZCLkoAycz4yu9Dmk").unwrap(),
    Pubkey::from_str("5PjLJUUsGnmpcqXnTXAk82wCPbuDAUL58muBpTDkciYu").unwrap(),
    Pubkey::from_str("5PjM5UdjoiF7RL5ofuSoMkfTaADiVtdt63o3uct7TVtQ").unwrap(),
    Pubkey::from_str("5PjH5q6dWsaDcEThFJuE5tFvHVE72rWws4mSD94mzzo1").unwrap(),
    Pubkey::from_str("5PjHErw7PdGnf2UwFoeH39dGqw9Lv6m4zgDoDU6TH9bE").unwrap(),
    Pubkey::from_str("5PjMmW7FcbgaK1N1w9ofbs8TDBPAuLBKJCCR28REMSoM").unwrap(),
    Pubkey::from_str("5PjNBH4LvxK47H25fc9V5wiPXKqRtqNL11u99krCGKjx").unwrap(),
    Pubkey::from_str("5PjNkM6REXFbdiTP3GHHtVVE54udwct4TZRebr9ftqW3").unwrap(),
    Pubkey::from_str("5PjKkbewV59rfJtK8XZSQme6DDb6FTfc1z1t9wQ7tNGx").unwrap(),
    Pubkey::from_str("5PjPnnzMCdNuZnNF4JfhdYypWQmLERxxeDMD9Sqs77Sm").unwrap(),
    Pubkey::from_str("5PjKpTv47U3UdNGZGUkVokePCL2DQu7yoyXnruKFLw49").unwrap(),
    Pubkey::from_str("5PjP7oJXoT7mxxEDepFJHkYSr5iYc2pgHpFgLbMgyu18").unwrap(),
    Pubkey::from_str("8hqZS26KSh4pR2VWDBc1qS9rTs7XktN3vTHWVGCDVuYu").unwrap(),
    Pubkey::from_str("5PjKLf5pPFeayZEe9FNhTWaqab2fdE1XjQxiCaqsHoEt").unwrap(),
    Pubkey::from_str("5PjLtKMmdU5Bq9Husg9i571R94NANBVhcYN7EmzwbYKV").unwrap(),
    Pubkey::from_str("5PjKq6sCg7F1Xh3wdQZtXJNH9X8bMSGGbELZiUezxafC").unwrap(),
    Pubkey::from_str("EvtMzreDMq1U8ytV5fEmfoWNfPhrjZ87za835GuRvZCc").unwrap(),
    Pubkey::from_str("A1enLcj9XmuVeYCQScEruwnfAz7ksQhbuGFUgvgeS1a6").unwrap(),
    Pubkey::from_str("5PjPY1FdqV4F3Xb8dKKb5yKdeNcE5y4GtnEfNvDWhR2d").unwrap(),
    Pubkey::from_str("5PjKzgewdhnFHtYVdTvFKUnrj59j17raPxwYN9TAY9Hi").unwrap(),
    Pubkey::from_str("5PjGMDxc9yossMRutmHwG271YiMAm96mdvT5koSvEHxL").unwrap(),
    Pubkey::from_str("5PjMzeZCLNGUyQzpowRRr7FjLZawhWL1vgugpbDVNdbs").unwrap(),
    Pubkey::from_str("5PjGtSUEzeybZ3iY9EVJ2rJjkSVm5sHYg9BuH2uPF7Ts").unwrap(),
    Pubkey::from_str("5PjGhQ2o3r9c35yABQknSxAuHbzWchdz5brBX33UTwrP").unwrap(),
    Pubkey::from_str("5PjMMEeUd43cEMdkkoMs9Y66bGbK8wykJXGCbBy2HXLY").unwrap(),
    Pubkey::from_str("Evwz4AatByRhePuZDe1zVt8Uau9ke6PWm4XNQczFdfTB").unwrap(),
    Pubkey::from_str("5PjKDXTQzAV1CRuQ35uiBubKwjB6tLftcaZDr3xaLPvz").unwrap(),
    Pubkey::from_str("5PjKbN1Sz7BEpzFZMmTHUmkHiuDNtb6HxATPerHGGqS8").unwrap(),
    Pubkey::from_str("5PjLd53HaGdd8U7p9w8faT2XQPTRtoY2zA8YTsw4hiHJ").unwrap(),
    Pubkey::from_str("5PjNeBznTkG34dUBN6bQL3syg7m5ifHv55bfLSsHmUUB").unwrap(),
    Pubkey::from_str("5PjH8ebDZyh4dUnAV8C6mu2YUFHkxwgBFkRMs45LbfE6").unwrap(),
    Pubkey::from_str("ACahZ8AwuxqAkK1sWgMbqRHPhrnMGErYe6vXy7V3886E").unwrap(),
    Pubkey::from_str("5PjKrJpsABRBEXgBgAiqVP597uDNt2YpdQkEd4wjeycA").unwrap(),
    Pubkey::from_str("4F6mgqT7RBCeTNMbHGgSEed7o4waRrZjXj4erE1T34i8").unwrap(),
    Pubkey::from_str("5PjPbzqDajrQAL6oUjQTMSLUjsrcKud9n5u7pKXC5urG").unwrap(),
    Pubkey::from_str("5PjGnRA4gAhDqwpQEfPXA9NxDoEjZEzrSBrBS8KGLeMd").unwrap(),
    Pubkey::from_str("5PjMgq3ghDAu6ZxNPbpKUxyxmJvLfjZAtnMJhe4NQCnr").unwrap(),
    Pubkey::from_str("5PjJ9hUF3iCXYRFF3cbq5FNKc77acDYEseWuhVkfwFCS").unwrap(),
    Pubkey::from_str("5PjKj9wF3oKWAsTr7Cfvu6gQbQEVEFMVHCawMvAAoECp").unwrap(),
    Pubkey::from_str("8G8ZrLBTH2ENUwmRFjdM19xL8nRZvbKMMw6q4YFQoK9K").unwrap(),
    Pubkey::from_str("GANbyXQSXz472yHhB2VR863d222XmKSiaJUJU36rYMV6").unwrap(),
    Pubkey::from_str("5PjGnMhB98ET6j33uaXtUaMnL1GTnUef3UceemHv5g4u").unwrap(),
    Pubkey::from_str("5PjK9b5QQ4Wxh2iwibYjf3HEEWtwzuF3jcq5nNiUqX8n").unwrap(),
    Pubkey::from_str("5PjHNnUZdcpJZdjsCNRKZrj6R3sWNs6j4y7WKZ1CXKzB").unwrap(),
    Pubkey::from_str("5PjPK847uHwqHF8boAdnDHP36aJ3WraCMw5WPEWx9dkg").unwrap(),
    Pubkey::from_str("5PjPStQqLGBZKq5KNVdY7fxJx9EwzHvnrjdEWDsDgb5s").unwrap(),
    Pubkey::from_str("5PjPbfXXFCXe8q1Ln8RPkKtTYUbkAz3nGgDcEWuw96dD").unwrap(),
    Pubkey::from_str("5PjNmo8FEk1BQuRAU6aXAgZn6pqtSsvMB86a6md9ZyTC").unwrap(),
    Pubkey::from_str("5PjJiQfgfzBwZE5CnGPV2eLt3LLNjgg4XTW96Zx9vrDi").unwrap(),
    Pubkey::from_str("5PjLaNPmGkS9nDPWtNtQJ88XHVnsKwpeztVjMTU1k9aq").unwrap(),
    Pubkey::from_str("5r3vDsNTGXXb9cGQfqyNuYD2bjhRPymGJBfDmKosR9Ev").unwrap(),
    Pubkey::from_str("5PjNpsfjMwX5RiSBGSWPbKftdGf3GkEGwdwtXysHWgkK").unwrap(),
    Pubkey::from_str("5PjPLWo61yxgbARmjLXs319EqtV4KPZ2FwprjxbGj7YM").unwrap(),
    Pubkey::from_str("5PjPRUWCxCcYv3jt2RFoquKB2jQAjijpJ7obnSHdn6pj").unwrap(),
    Pubkey::from_str("5PjKjaLtByUQsKZYgpZsv9LgieNo55pbaMxXj2HCq4ug").unwrap(),
    Pubkey::from_str("5PjHXCrxQecY9tvE4LGUg8G1VNqpJsZRjDMfnmfNVJ7e").unwrap(),
    Pubkey::from_str("5PjM3egQxPiAYCt1kj78ALugzkuL81a6j8j2LcqgnP6z").unwrap(),
    Pubkey::from_str("5PjHz6gjQnXkKLWmmxYYEmPcgf8iVadgdftsnenf91wC").unwrap(),
    Pubkey::from_str("3QaNhP4vT6PG3eoQwg2DRbH9ecmy7pR2f1PBPWCwDBYd").unwrap(),
    Pubkey::from_str("5PjMN6KHmceRTthyhvPMKpzAFpEJdNQDawti3dAZ7iDp").unwrap(),
    Pubkey::from_str("5PjKFTJp7iZGx6vnWrNLs2SueBnvhgVzBLQzyG6WUnkc").unwrap(),
    Pubkey::from_str("5PjJZkpSNTLhyLFRN8PnjxPPHGicoxM3DCnTGUmgrios").unwrap(),
    Pubkey::from_str("5PjGhfJHHSUxJkggKeUiKsA4FhRECBgsczeTqMhftGdp").unwrap(),
    Pubkey::from_str("5PjPHuLo8RU5pC6qT1wNPRM5W4niGmQeaYDVxhrwpBW5").unwrap(),
    Pubkey::from_str("8ER3VKcuysXDoKTgGQ5XQoxWg9RJHzdmSGgGWMbBTPS1").unwrap(),
    Pubkey::from_str("5PjK4tVbBzfagcv1GJEPHB7RytC2hxtsnDJ6QC4YfqDZ").unwrap(),
    Pubkey::from_str("5PjLvEggQzrHaAn9dKDgrSQVomurYX9Ax79WrQ5wi9x7").unwrap(),
    Pubkey::from_str("5PjLaVzgMPCWzRGn3MYvSWfMLEmG1MBd5pyaQEndT9e2").unwrap(),
    Pubkey::from_str("5PjLwZGm9dkJ3tw1FHjoTbBsqNEfjgVgf8VHvmfrWmT5").unwrap(),
    Pubkey::from_str("5PjPWtz3NbraFT8BQXBXRAD3W3XWbfjZ56wMbnGk83KB").unwrap(),
    Pubkey::from_str("5PjNaRX5FMkXWpHW76374vnWDMjCNFYXdj4rbrwskpZf").unwrap(),
    Pubkey::from_str("5PjGkZKF1zbFqEgDN25dHTR4UFosKfvjnPeEYKzmPSST").unwrap(),
    Pubkey::from_str("5PjGwvv6tx6T7DoQTKpgVahbFJAzhownS5BoxuFUqcQH").unwrap(),
    Pubkey::from_str("5PjNB1ecWHicPLDeGcxsSdzcgZJFScxZCPaatPAmBvqW").unwrap(),
    Pubkey::from_str("5PjH75C6cMTevcipsRChsmHqg4X8VZvRDK8U4Ux31uj7").unwrap(),
    Pubkey::from_str("5PjPfZ9eUSGRmcRuMGs6kZJvbKX81ts42F1ByJEK4W7x").unwrap(),
    Pubkey::from_str("5PjJXwTtfsKbTXjWtMpwq6czs1RRJZWP8hyVofMQcC6j").unwrap(),
    Pubkey::from_str("5PjJ8gBBupR8qifYTLUuCqd1eg3stUD8AApep3zBAsdJ").unwrap(),
    Pubkey::from_str("5PjLKJ7ETzW6MLz9o8i2mMvuvEJu3Fcp6YjpuMrTvZ58").unwrap(),
    Pubkey::from_str("5PjJa1PVtRpDadSRaD8pNFQcYeSeV33RGeFzY7vA68Rt").unwrap(),
    Pubkey::from_str("CUJorkzjpvZnm7ynSaxuwtK2MJk21ReFDSF4uNc3xrsc").unwrap(),
    Pubkey::from_str("5PjJvEkBnp612n9npeNjN1oetYp8TwuecAW4nv1xw7Sw").unwrap(),
    Pubkey::from_str("5PjMuymRQqoB4wjRYrL8agcoZgyYoeGhAawFsi78y618").unwrap(),
    Pubkey::from_str("5PjPfsochd665JhwxsWJeRGiQ2nihfywFmc9npHC36Nw").unwrap(),
    Pubkey::from_str("5PjJ1YmYSn4pyfG5TnjRhVN5eGmPFV8BZnv5ewQWETqW").unwrap(),
    Pubkey::from_str("CdbgqE5B9oADrSAWc51Mgw6c3B6nvYJ4c431rftpoVqZ").unwrap(),
    Pubkey::from_str("5PjP7DLTA1jTyYT3txUFc5dKvUiMsBRFQozSeP2AHRRv").unwrap(),
    Pubkey::from_str("5PjNzADACZgj1PtxH5qwDxBwTkDjiFF5g1PgNtcnXWab").unwrap(),
    Pubkey::from_str("5PjKePeXJpcASvrjp76KL9ToRCJYobxAKWD7jFEqsPdN").unwrap(),
    Pubkey::from_str("5PjHAPPyr81Q6X6U25eK1RjnonypBnkHB5jnzVHWbAPD").unwrap(),
    Pubkey::from_str("5PjGtKYqAjyGvSdrzvom3JbikVgtatVmnpdDF3bi79xk").unwrap(),
    Pubkey::from_str("5PjP9fw4FGFkpihFLgw65zELYPakhHTadKZPkWzrJBw3").unwrap(),
    Pubkey::from_str("5PjJ8PM4jwDy852i9t2mQhDXwgt9Ewe6Mj7fXP3AdFAq").unwrap(),
    Pubkey::from_str("5PjJdNyjB5F7PSximCgdkHTzgar4dJEduQynGqtLDoiN").unwrap(),
    Pubkey::from_str("5PjNsoXBJSYfcV1AaUrrVTxeLW4yzt7gZjwVPbMLYu5h").unwrap(),
    Pubkey::from_str("5PjNKVU3AUKBSHumMjqwbrEBdVEZa8taE3qmt4QgqeXE").unwrap(),
    Pubkey::from_str("5PjMvs5H7jAcdH7sFLWu2Nq7UZ6Vm9bwqHSrvsnNjM1B").unwrap(),
    Pubkey::from_str("5PjJkJ3wiLnKb4WcMNUtiWNR8rrh33spCtc4thMbQu4H").unwrap(),
    Pubkey::from_str("5PjJP3xSfnnnEdTY6aCDRe3XUHWg55wHWzAPR1GJyQTT").unwrap(),
    Pubkey::from_str("5PjNR8Sa8CbMxG6KZWL8Gb38XBv2fXGYb9TSHoyvvi8R").unwrap(),
    Pubkey::from_str("5PjHkyGG9DLUYFEjg6YE9JJMKyp71g5kpUuk9i7tURkA").unwrap(),
    Pubkey::from_str("5PjGnNRYGs5YLFWk7zfGfVkM9EkJqH7iznXK9wc5QS7G").unwrap(),
    Pubkey::from_str("5PjMxaijeVVQtuEzxK2NxyJeWwUbpTsi2uXuZ653WoHu").unwrap(),
    Pubkey::from_str("5PjJKzjhqgNWYPE3PMnkHUfP8yYJeNucxL4bHPTQvsbn").unwrap(),
    Pubkey::from_str("5PjK23PobLEfFtbZCE17ZN9FynGzrv5AZGcLSWKq6SAU").unwrap(),
    Pubkey::from_str("5PjKAc3fsHGqSebwvfhNh749ae36Dke51dPN5r2bxNrU").unwrap(),
    Pubkey::from_str("5PjNKkg2G16La2g4QX2rqAs8p1TM2KUtefRtxKYwpv6g").unwrap(),
    Pubkey::from_str("5PjLTtK1wNRVpDgFA99w1B3ZPirJTWcsaKtrKuqjJUtM").unwrap(),
    Pubkey::from_str("5PjLcY2qMK8q8xuebEBaLfPjcK3QDxTCT9TaHMTD9bU6").unwrap(),
    Pubkey::from_str("5PjNKZdfoXX4ZE1n7kqe4BmqLVUdpFcFcDWTWKKPnWEQ").unwrap(),
    Pubkey::from_str("5PjHZi2D9H6Z8zqhyg4LgaXtCx563gKMimpPuruCYnQ6").unwrap(),
    Pubkey::from_str("5PjJHwRAL7ZERCRTySeHDDiNcymwgMS2JSLPaCf62yy9").unwrap(),
    Pubkey::from_str("5PjNdKMLhHdYWPtwRyBPsiRkY6P7po99Cim4j6Be1Kcz").unwrap(),
    Pubkey::from_str("5PjGMjyqzSVo6qY3nCSvTBbJgicr6LPHACYfpSqM45LW").unwrap(),
    Pubkey::from_str("5PjLjSDFsVwwjxuphu64KCyPQundRA53KJgbnWwQ1pDz").unwrap(),
    Pubkey::from_str("5PjMhnd9tQw5vpkTn9p8XGpuASz6257ygYf7dtgcLLX5").unwrap(),
    Pubkey::from_str("5PjGrNoQ1peC5kezHhdXaarG9tvmoSTdSUqckBcyiM54").unwrap(),
    Pubkey::from_str("5PjLDDXXh4mybzTd28MoLm9u6FgsNMkXctz7QtYJLsPC").unwrap(),
    Pubkey::from_str("5PjGNJPyJtMctBioXUG36yUpz27KSL55Y3u35w6xksti").unwrap(),
    Pubkey::from_str("5PjMoL7BHC9iExbpithgKvNNadhTAzMMb3GtorTKKDAB").unwrap(),
    Pubkey::from_str("5PjGvyr9ebCqHtmFXsvpqk9od4DUSCijxzi2oCnQ51Bp").unwrap(),
    Pubkey::from_str("5PjLhKhqz41r6kCS95JD3FznVP43BC1j16Z2h21LDrLp").unwrap(),
    Pubkey::from_str("5PjH5k5W5keJazAdHGiv8hzqeRwyQ2PkMH6bskYMNGHp").unwrap(),
    Pubkey::from_str("5PjJ5cKLywdDdvasiBkbeocCKdQDgpocT7REg2RVuC3E").unwrap(),
    Pubkey::from_str("5PjNZdmPQyG2pHr21TCQ8cQCFMg9fdDt32TTZ4ujusK6").unwrap(),
    Pubkey::from_str("8F6NCo1PiakW7m3eeEZvdxsjXF5bkLD3QZsTxaNg9jvv").unwrap(),
    Pubkey::from_str("Esm9CAT8FFBgJM63iDLRntKq3CjGZNEGidNEmcN4XVby").unwrap(),
    Pubkey::from_str("5PjGHKrzk8nrWZ2A2dPAV5XsWVm1VC6CrMsoFHTdRSzB").unwrap(),
    Pubkey::from_str("5PjK1WavKLEEVHG6AjuXRJ8z97rWJMuph3BXXJrKuEkD").unwrap(),
    Pubkey::from_str("5PjG8w31DyYUaHXHPdfGHEqcSpAWFdWprYp6fTpMrjyN").unwrap(),
    Pubkey::from_str("5Pj1kicH7V3EpDvxmXUMaBeLSnvy1Ac1wQTFVSKBDnHR").unwrap(),
    Pubkey::from_str("5PjK6m62EnSxp8qHUnYN5kVpR6pQvpoduV5sC2ZGRSwp").unwrap(),
    Pubkey::from_str("5PjLmEDurEUb3iZ5GdRnQqi6XBE2SJTwSWbnCiAhNb4n").unwrap(),
    Pubkey::from_str("5PjHiaubj3EivhvVaNaEtXNp2fHgsiwQtjx7ujDXuATK").unwrap(),
    Pubkey::from_str("5PjMTdmM3hUJtvzfAd8rczUJZCgvm317rQRbT95obXWe").unwrap(),
    Pubkey::from_str("5PjKSXSAhqAg4SoBFtdn9iLdYSt5ddRZbBW16sJ3xmmY").unwrap(),
    Pubkey::from_str("5PjMiGsa7LHJJhmNba8srLw6DfkSTDDQmisYtppRByyW").unwrap(),
    Pubkey::from_str("5PjGYVbM4xtiE6STAUH2WnGAjXn6UTet6JMgybRho7Np").unwrap(),
    Pubkey::from_str("5PjKa4uh7agYLjFSgRo2ENsjU8BwQmnkgk2Dcuiu5TuG").unwrap(),
    Pubkey::from_str("5PjKDvoroHTeDcLUrYcCdEByFFWadJ8AKxeY7DJdirHb").unwrap(),
    Pubkey::from_str("5PjLnbV564sUgqd59s3zN6cnJQMyKRppq5Hsdtk9pUnk").unwrap(),
    Pubkey::from_str("5PjMS6rmwCJoEHgzicov4Qyig2vynoXg5eo75gMGf565").unwrap(),
    Pubkey::from_str("5PjHh4ohn7Fji6R4Df3KR7VubUUygC8X8BUJvKXig6vj").unwrap(),
    Pubkey::from_str("5JvU7QoYiZDLoWro4kzDenYMNpvQsGTHhYhv9b8NzEpf").unwrap(),
    Pubkey::from_str("5PjJ1RcbPNKeNUMpgw4z8chV98LQmDxuvttc9d33tiWR").unwrap(),
    Pubkey::from_str("5PjHs1d8aKUx8aaDA5qqu9pBsmwrKThTpn6uERMNZYUF").unwrap(),
    Pubkey::from_str("DuHRmA6Dc9L9TsoxcfYFuuu4Gt9U89ogv61ewbhhbKRP").unwrap(),
    Pubkey::from_str("5PjGmWsYsvygM1DwSLL7rP98iUdRKZFSwHcrmMyQEubq").unwrap(),
    Pubkey::from_str("5PjLhTpUgdudeQDV1RjQyxFyBftQ4DzejhSttjbCZgiP").unwrap(),
    Pubkey::from_str("6MwFJtXNgPsqN7RAhtgNDrLCKoof7SJH8E1KqJArUCqq").unwrap(),
    Pubkey::from_str("5PjKW2a5UGcuaD784pCbXkPcrC1NWEcN2CXro26XeuMf").unwrap(),
    Pubkey::from_str("5PjHrniktpF72ZAyi4ddVrXdBmraY7VQFDVXuKgW7hfN").unwrap(),
    Pubkey::from_str("5PjMjwW8NB4kxduAzr4tDszZh8Ai3DQLMiGQAaHFczy7").unwrap(),
    Pubkey::from_str("5PjNVKw4xxEFGfLhRgcq3FLMgwARv1xkr42Pxag7aLnA").unwrap(),
    Pubkey::from_str("5PjH1sFKAjWuqu2kaP94uTrLEmmbS92K3yGRWs4pCKaJ").unwrap(),
    Pubkey::from_str("5PjG6VacBXnTSQ9cRoJU6osyFAuo1xMrsHZH3TtAZToA").unwrap(),
    Pubkey::from_str("uPrmw8dKecH2kwB4e1zuep1sahcKfvhFAza6E7uYtjC").unwrap(),
    Pubkey::from_str("5PjH249xoiTFUBM8iq8ULfgFmM8hpdUM3TUsvqAiAkKk").unwrap(),
    Pubkey::from_str("5PjHWVNUovz8y1i7eYWfCmkYDzMUQr7k7gXFAVsA6PhF").unwrap(),
    Pubkey::from_str("5PjLBuhrCBABtkCrtL1iRqCGba6dnj7PTJ9kAxoQfc1T").unwrap(),
    Pubkey::from_str("5PjNsGwoQs3LrnNytkyGCu43jqzanQWjPQhuK9ZPTLTw").unwrap(),
    Pubkey::from_str("5PjJZiXSeEQD7q6CRW2Yek9F4PovkKionwHTnRExQzUq").unwrap(),
    Pubkey::from_str("5PjMqMaof66pxXq9mzgvmPFArAJDqPiKd5bQjHAfDcYL").unwrap(),
    Pubkey::from_str("5PjK1No5Fx2SJW7ha48k1ersny2EoDquNEPdminVQmEX").unwrap(),
    Pubkey::from_str("5PjHcToXiFg3AfRh1kf46vvhHvTHn6RCECtAdWdzq5XQ").unwrap(),
    Pubkey::from_str("5PjGpdnVo6dKvcVFe8kHLdTjwgcGr5QcJLGuNgFZ1tAt").unwrap(),
    Pubkey::from_str("5PjPXHzLwaG3PkhAf1E5pkfiUa51h979jb9JcT9uSLh5").unwrap(),
    Pubkey::from_str("5PjHcv95jLbW6X1QUgv8zYNyzd5gLpwirfURN8K4KX92").unwrap(),
    Pubkey::from_str("5PjPFLUR9K1kbAnxXdrp5Bf5sRSoiGbEtDa7J4bznAY1").unwrap(),
    Pubkey::from_str("5PjPZ9e86boZGp3VNMac9Weq4zwroNt3oxTU6TBQFciS").unwrap(),
    Pubkey::from_str("XbkV9HZpLdv3CjMUfoq4t8nkxR6UguHb4oP8aAKBGV2").unwrap(),
    Pubkey::from_str("5PjLe2nuzrMqGswZYsaszd48WAv9sM7UnDaszLVMGsEB").unwrap(),
    Pubkey::from_str("5PjLXiQ1j2Tw3NLtDm8TYkB3zbQ7Hdop7K3L9L1MkE41").unwrap(),
    Pubkey::from_str("5PjP5ePMnJPFkeGNqHD1rCPKSosTrSNPGaGUctDpUr7r").unwrap(),
    Pubkey::from_str("5PjLfzPut9oSP2uJSzFToLP3deJ4P9aNuaS2pSygets7").unwrap(),
    Pubkey::from_str("5PjGB8FFt9H7rpGYPsKx4ykmediZWEg8QuJvqLZQEu16").unwrap(),
    Pubkey::from_str("5PjGWwcLiWhCMihHS2PvcTR64TPa8Lh4eWs5d4GnKnSu").unwrap(),
    Pubkey::from_str("5PjNXWRFydReLACN4uyneuqgita1DBKaAenLmJkxnvnW").unwrap(),
    Pubkey::from_str("5PjHrrZSjXztE1osswkEVRWtJ1zJnZsTrPEpDq5TXfXG").unwrap(),
    Pubkey::from_str("5PjKpdXALkbA1D38NTDnGGFtQhc16wC4Qt82n45sv8Um").unwrap(),
    Pubkey::from_str("5PjLaDSqSVNwMvWvMgEFACdMMfNqqvLYVCJ1to9stuQ7").unwrap(),
    Pubkey::from_str("5PjPZmvrqov9Xbafvq3FL54XU65hykkX29r6Rp2Rv6pV").unwrap(),
    Pubkey::from_str("5PjLyxCdU6RmunxrwM6UDHtdmjbFqZg9etNZPRdY3pCa").unwrap(),
    Pubkey::from_str("5PjMJgFFJGdJHAh9BuSkx6pzJ2TkGkRpPnAGcyPkVN6E").unwrap(),
    Pubkey::from_str("5PjMNskPkuDiSb3dEAZnAyNbRaza2ubzfkTgiW8CFPFw").unwrap(),
    Pubkey::from_str("5PjGPQG5QPrH242cpm9hHAk1BBTV5bveWk12LAhtCkY7").unwrap(),
    Pubkey::from_str("5PjPdcUgbZ9DCRBSRP2zAiCcfL9nZi2BJUquFZ3Wv6zp").unwrap(),
    Pubkey::from_str("5PjM6bEPjAkcjgKV1Dj57ZsZNruu5u1XSAtmmjApSbBJ").unwrap(),
    Pubkey::from_str("5PjKqvnD2qHqJJPVHx8Z3MfmUgXokSVESai82FadKyB6").unwrap(),
    Pubkey::from_str("5PjMeKkBRhv5rLFVuaBg8VAFkNGxi9qRBvtdun7dfvgh").unwrap(),
    Pubkey::from_str("5PjGDigNf4EM5P2TDcaLDAQBqgqgyzhEyjDESJ22h2vN").unwrap(),
    Pubkey::from_str("5PjJE4NvMJzjaQqAaFHCCweAhrV2kSvKFjcMgNWj3eGn").unwrap(),
    Pubkey::from_str("5PjJHcGwtRv6sqvhV3A7fKwZgRdRx7n88ZWDWbEu6ADt").unwrap(),
    Pubkey::from_str("5PjJZ8Dfrp956uyeaSsPVSScTWGNgVPCfbKp8CtnTu1A").unwrap(),
    Pubkey::from_str("5PjMsSLZ71twpafTzXvjmW3NhGZA1WgGhXJVpRJvbDWy").unwrap(),
    Pubkey::from_str("5PjLJx9S9dWKS42PmWcan9T7WKR9pZ5CHJXfMrvqBNE5").unwrap(),
    Pubkey::from_str("5PjLcJq9JR1PBKLkr4a1isD2BmADjX4ajncYcUAqB1XJ").unwrap(),
    Pubkey::from_str("5PjJ6nwrMgdgKUCu1zWLHiE3EHQmr2q2eMPNxSWAbyaR").unwrap(),
    Pubkey::from_str("5PjPoqXGoRqeSFeWtWFon2WGS6ivwabeSgE2wqKfoCsU").unwrap(),
    Pubkey::from_str("AqbAM68p3cx8w7hc16QAFqqwC8FLfQPPW1iKxxcjVgYy").unwrap(),
    Pubkey::from_str("5PjKJgKU4gR27DUsXfuKqyhe1jz9wwKCmj4A6LCQGfK9").unwrap(),
    Pubkey::from_str("5PjKNL52g9UWHxncFXkUFdpzLJkh6CDXWWb2aFDr9fxk").unwrap(),
    Pubkey::from_str("5PjKRs765KcCoQFyM6zcVxMkQ23WaQ39yJ5pGDTMfYRJ").unwrap(),
    Pubkey::from_str("5PjJbJsJvpCfmunbX87XQ7HyPM3pZnqaKvna3aMhYj3g").unwrap(),
    Pubkey::from_str("5PjM4nvbSbAUR93VUBWDaawBfXL3Vs1A6Anj4cqUGkgN").unwrap(),
    Pubkey::from_str("5PjG8yVnyVhJaEYMJbDZ2xqZseooWET28RYnj8aaWLSM").unwrap(),
    Pubkey::from_str("5PjGwohcmKQMsfGBhyA8dVNkZuCSNVNtvY1ZWT2RfVSq").unwrap(),
    Pubkey::from_str("5PjKhd57Y3edCPjAi8g2tYin6v8rWg5tgtbFLoMdXJC3").unwrap(),
    Pubkey::from_str("5PjNGUJCf43TyLu8epnkTPGPaPgDYZwxnqU9Nkxsrvxu").unwrap(),
    Pubkey::from_str("5PjHzCrAFMQqw63VuLdNs15bR7gCRuqfGLXY5fbcsgmV").unwrap(),
    Pubkey::from_str("5PjK1pT77m3guMobMu47SvghVBSFXArkcpNvddw1qz1H").unwrap(),
    Pubkey::from_str("5PjKYbyGq9ZiKnqDEhL5F5nsNGEU31Hw18jnw9eETSf1").unwrap(),
    Pubkey::from_str("5PjKeH6BXPJ641aEcmzVfauHskdhWhgKoig1axt8ngn5").unwrap(),
    Pubkey::from_str("5PjLSzFhr1fRTcf63rXNijseRr81qKGw2SEPMub5GPTm").unwrap(),
    Pubkey::from_str("5PjGdwD4dWJKr8VXcdNLh55ZtGufi8XzUNWY2uxcSuig").unwrap(),
    Pubkey::from_str("5PjJAFog5MJrMVLSad4AV8zZwDC1gN5ZZFQtoJ65U5Xn").unwrap(),
    Pubkey::from_str("5PjJfXZagepk31V1u5tAYnCfqsrbVFsa7b17PPLAnh5h").unwrap(),
    Pubkey::from_str("FUBad9ZBZmegSugRt7qY4uM1yRxzhBRpywwkn4mRQAeQ").unwrap(),
    Pubkey::from_str("5PjPkMY8w4vmAwhr8Hi6uspKZF1EXi2fLRYHzeJEhzmU").unwrap(),
    Pubkey::from_str("7bqCZ88nK4nU3trSFYZ5TAoTVQ3GLsBuKE8CBxt2nUe2").unwrap(),
    Pubkey::from_str("5PjJtkoKw36kmQ8qG2kdtyGQ7arNtzwGgT39rSUpeHHp").unwrap(),
    Pubkey::from_str("5PjPUhrWncCfGWMktfhxVC1K6UkSupGXrNttakB8z46o").unwrap(),
    Pubkey::from_str("5PjJvu1ZLYtAybWqKgdHd6LicbGexfyY4y4Ejb4ok2A9").unwrap(),
    Pubkey::from_str("5PjP7eqDAnevbeGKr8bKGuvRGEmgUE1q9p2MKiC8c5qz").unwrap(),
    Pubkey::from_str("5PjJ3gRx5bUjPcpagrcM3cwJi3XqQ8MwFtrZFGKVJPjv").unwrap(),
    Pubkey::from_str("5PjPUVw6tR24rupUdYsQcu7oQENTbRZyjXeKSFTUfJZY").unwrap(),
    Pubkey::from_str("5PjKKWcJs6w2J5SQqG8FeKUJ37e3X3A3nxRAfdgb8PL3").unwrap(),
    Pubkey::from_str("5PjKKXX3EjxxjpcGMMe4vru4kahetzesjBV7wzM5TP6v").unwrap(),
    Pubkey::from_str("5PjJLNS3DVgpipCrxCwJY7rWpj1gHMJeywjxVJNyTZnV").unwrap(),
    Pubkey::from_str("5PjGV9A8sDeTUwEu58amnFKnUWqWjh3ctr35C7maTVky").unwrap(),
    Pubkey::from_str("5PjJyoP4cxSkxLvvtqxnNgxYBgfFRxD3Cgb2ZQea3MPR").unwrap(),
    Pubkey::from_str("5PjKr6DD9K5dJeTWnDRjkA1ShwQ7YpcHbiMLTkEhorVu").unwrap(),
    Pubkey::from_str("5PjNUtxa8PKbsBuVkrsXWExkN3BA1pCLruP599zbKM88").unwrap(),
    Pubkey::from_str("5PjM6X633xjqX3cReKeD9drDRo9nazM3LEwckCoJhkQC").unwrap(),
    Pubkey::from_str("9yM42HMJnN69rhMGr8nCYSRtFxjWTWm5Z6GeucyLBEHg").unwrap(),
    Pubkey::from_str("5PjGQ3F1Gs9vWX43CfD1ZSvaK6rqkFSxDoSjBMugbrR3").unwrap(),
    Pubkey::from_str("5PjG7qUPLrdv3cVEdEZqpYVqyBa2myYzHsifSZycmhRN").unwrap(),
    Pubkey::from_str("5PjNLuuWcopU6kTGQHL15fg1iTzPCYvxpRn9yQLT9MnP").unwrap(),
    Pubkey::from_str("5PjGAzgraAKJhYkkfGnemMr6wrTsb7v5Q4JhNQMzumuW").unwrap(),
    Pubkey::from_str("5PjLLr3TjFn4VwLGuXGNWPVPiS7EJahZguStBSeBFFrs").unwrap(),
    Pubkey::from_str("5PjHckeWWQTLv6CS8CuXbwvKqDEPjfycwCuDQXeZ2yof").unwrap(),
    Pubkey::from_str("5PjPb7HAMdgES8yJ2G4Po5hQPWqnkTKeS917i46s2z9f").unwrap(),
    Pubkey::from_str("5PjPeWhtoZ8ghg4AtKJ7mZvt3g8DrSqKWm7dJaEGvJTx").unwrap(),
    Pubkey::from_str("5PjMgWxuha8zMxMN32i57AnMC7BALLcMWiAAXpEvzjkX").unwrap(),
    Pubkey::from_str("5PjGqEtEDDq3CKbjhv9DDNfhcdBLpfQi98Q1UubZcp46").unwrap(),
    Pubkey::from_str("5PjLknVAYrx9GNtCyKQc64odFnprSdCkCjv5NyJbUoc5").unwrap(),
    Pubkey::from_str("5PjJ6fsrFC4GuMmm7sG3e1gcTLB3qRr7aJZASm3ftRJe").unwrap(),
    Pubkey::from_str("5PjJXwjsnvbfN7eQLxZDV2aVa28xfgr6tmvYnwiCMz8n").unwrap(),
    Pubkey::from_str("5PjJVYiTgGapv4SGSh72taT4QdHsZf8NwcEeUBxpQ6ys").unwrap(),
    Pubkey::from_str("5PjKbR9qMgTtQt5pe3ACBRVnQbuUAyx4M4NNGUkGqMLR").unwrap(),
    Pubkey::from_str("5PjMinT32HUbQrSGmbTHCSw1UyE7QdPtDZYar1HizVjj").unwrap(),
    Pubkey::from_str("5PjPRuYDVcWDXRUBsvEXJgBDP2Gvzte8SQkWiKoJtNry").unwrap(),
    Pubkey::from_str("5PjMvFQFSGZN4YmXBUsz48h97muAXqFuegqp22SuhU5j").unwrap(),
    Pubkey::from_str("5PjJAY6Ro4LDCpcF6zRTeU9HPoS1vN27PYJFCcB32Z9a").unwrap(),
    Pubkey::from_str("5PjJo897WTDaretivD2wPDjnq6zmtAK5h7fypPVU2Ucy").unwrap(),
    Pubkey::from_str("5PjN6aLNhK3UoX7d4w4GNp5AUNr7sf6rgrhezeevXp9B").unwrap(),
    Pubkey::from_str("5PjHJhAv31qiengHWHkbg7z8ks6zmMRW72dC7K5EgABZ").unwrap(),
    Pubkey::from_str("5PjGgxhdPmAj9ACji6Lfe7y7eZkGEEotU17CHUa1rxpA").unwrap(),
    Pubkey::from_str("5PjPaRCdSJP36A2MXNWJGeC5vTLspZQncSUSLJj4R87Y").unwrap(),
    Pubkey::from_str("5PjMC3x5PdCZmrS6mFE2VcxAr6zUnMUrKWQziJCproNX").unwrap(),
    Pubkey::from_str("5PjJ4kZkt3kpptpttRem9jhmXMaxEw478SMNNLCg6f6j").unwrap(),
    Pubkey::from_str("5PjJi5Lh5DGH1AGtS62e9BT6659Wb4MCWiNe9PEqLwvH").unwrap(),
    Pubkey::from_str("5PjNcMnJY3AdoRRUuqm2oA9HbFoxAfsCYisQMPdNYowE").unwrap(),
    Pubkey::from_str("5PjGnJRFou4UjQ2RZMeUaZwwAXZxU2Vs2wYiyre2h8Jh").unwrap(),
    Pubkey::from_str("5PjL6jcpbK2oen75KmS7HN1LAiHJ3uympNAv8ApBTyrH").unwrap(),
    Pubkey::from_str("5PjMifDGg83fkK3Lbup5qmc9zHmRPfFJQLj7zN1iUC6v").unwrap(),
    Pubkey::from_str("5PjNax3UJtuNp1AMhEWjKAXmueQNEmu8gW9Nbd4uqaw8").unwrap(),
    Pubkey::from_str("5PjNP95nMpF1CuzK6yDWkPVSGkAAsJRju258a8HBSLNv").unwrap(),
    Pubkey::from_str("5PjPUoct9U9DnEJ38zs6tesYXVHjvDPmE3nobBAC1LEm").unwrap(),
    Pubkey::from_str("5PjMPJ4GJHcB3oioAxb8n3vH8B9H7k7hSKxNnXCTManh").unwrap(),
    Pubkey::from_str("5PjHytKV8RCugb6Vs9YvK6KHbWRDTYyKxDAujFBNXhBt").unwrap(),
    Pubkey::from_str("5PjNaFFtxE21CLqQ7ezHmzBtQ2ek7NFksAF2fUvTE8yo").unwrap(),
    Pubkey::from_str("5PjJUPp6AfuEnALgUGoCTGRJELNnJcbsgxSSr2X5hjTD").unwrap(),
    Pubkey::from_str("5PjKeKSuFRbiV7ageku3sdKYnLydx2GpLEUqakscKeWr").unwrap(),
    Pubkey::from_str("5PjNY5j7JY1WcPonEBUB8EbFsgs9fdrw62gcVTW61prJ").unwrap(),
    Pubkey::from_str("5PjHJfBDCXGNL6F3rNvQHUghhCLQANX4m8AGZN1TKUrj").unwrap(),
    Pubkey::from_str("5PjMAirq9nY7Vm4jn1H7T3ffCBXjv4FmQpKBP8krk6EX").unwrap(),
    Pubkey::from_str("5PjMVXLKue2GDSDfPzCSgPaXNuVnruTrbewC9SQRMYUz").unwrap(),
    Pubkey::from_str("5PjHt9ey1QTYfqcFG2H2q1VRu7tToRYuJ3QD7ssi2jro").unwrap(),
    Pubkey::from_str("5PjGSTuhvB1hzqojd9r4CwErMYNG1ERbwf35reQaS2FN").unwrap(),
    Pubkey::from_str("5PjH7gy16DwaKecR4iTi7KcgMTvWy45sxPsYEkkk2tTj").unwrap(),
    Pubkey::from_str("5PjGFAzSophyMAZXogGMTdvGTceD3aojyuma9D7ZpAdv").unwrap(),
    Pubkey::from_str("5PjNhSTVVWDwcGZzRq2byqhv7nZ9ZUx2vvMwMmBRiRTG").unwrap(),
    Pubkey::from_str("5PjHr7bc3SxT2NZpYTk9VZmWwKv7r3tvEHJDDUJvmKgV").unwrap(),
    Pubkey::from_str("5PjMKye8bnJSo8vphMJaVrydbKMSrCoK3uk5wnyt8fhW").unwrap(),
    Pubkey::from_str("5PjJSnRJMUxpuPQFrq8oJMDfwaCXTLSK8eitkMGBevkJ").unwrap(),
    Pubkey::from_str("5PjNfcC5VbSAcHDJa18WUK7aJ612EqM2hEEWtnp1dGEB").unwrap(),
    Pubkey::from_str("5fhDMuGKRDPWVWXf7BBEwifRFrp6XwXctDQoG7UHGVt6").unwrap(),
    Pubkey::from_str("5mrUN7rG14e88H2vccp3z1zHvc37DDMhD8DELMPftSp9").unwrap(),
    Pubkey::from_str("5PjHxBs6aXUEoGQmEUjrjtAVcRG9t3wEs8TYvpj1avAP").unwrap(),
    Pubkey::from_str("6FPiprpfDmnLkmrQiiHrGWeHTf8tRBUjKT3jzX3DPbP2").unwrap(),
    Pubkey::from_str("5PjMdKD7QSgqnUorMZWKKybob8A17gcMAoTNhRyLn32c").unwrap(),
    Pubkey::from_str("5PjM1ZCLoFJy2bcqoaaDNAevrpWNtT7A4N2SYJcLNGFL").unwrap(),
    Pubkey::from_str("Bpva88AdDL3Ct3YZyhqigLumMPcJozfRPzBbQkGry7AA").unwrap(),
    Pubkey::from_str("5PjJyME9xxgqSc5Zk8ZDEvhrArMod4ceaYRcLukzcZvA").unwrap(),
    Pubkey::from_str("5PjKNFm3rBhszknmKVtcY2tT6ZaJFi5EHhvwNsJsYGMg").unwrap(),
    Pubkey::from_str("5PjLHsZHk3d8QPPnMoGj5iRgnj56j6qszWSifsC8n9Y4").unwrap(),
    Pubkey::from_str("5PjLLFRhydNZy7P2RSJqYb2C6HVZFzcbdviouiJuAotv").unwrap(),
    Pubkey::from_str("5PjKaxCfAFRG9tqpWHLoH4fnqUQqim8tjzvELoUdy6Ek").unwrap(),
    Pubkey::from_str("Ayddz1UuQMKcYWhNTYAVaE2BATWNqD9yruMT83mk9ezs").unwrap(),
    Pubkey::from_str("5PjHb49LavX7FLXXw4Ha9KUikHSgvwtcf3pX4uRE8QjY").unwrap(),
    Pubkey::from_str("5PjJnE2ancYASoZUkFzBEr6v4WFMUoqaAF5Uyerpc7bG").unwrap(),
    Pubkey::from_str("5PjKsahDYuamcEsQnBxo8FEnf8GtsaQHgL8UiYZwpNzp").unwrap(),
    Pubkey::from_str("5PjGJFUL8vuzVC2VNes6zkGRnwu166tAdrkZ7wYdNhY4").unwrap(),
    Pubkey::from_str("5PjNR1CdvPmD7yGjVdrfEVtsDt1XqGq3DMXdFCsYLUTj").unwrap(),
    Pubkey::from_str("5PjKJFaJoFc8s7VkCmPFgpS5N9rAW5PLzxwqTfJwCwaD").unwrap(),
    Pubkey::from_str("5PjMA27LUKoDE6rGqdkYnsBDZASRhoA6rT1bAz49RqqK").unwrap(),
    Pubkey::from_str("5PjGTaWc2omJCeMcJTJMf4akYnxeo4upQFV3pEsCdMp8").unwrap(),
    Pubkey::from_str("5PjKhba6LYThqUwvYrX47SVcQqrHdbHQiCAwpVeubv2g").unwrap(),
    Pubkey::from_str("5PjLvhxa49E4EteNiXGu2LZmauKwv36eaCX8AsTQNCrG").unwrap(),
    Pubkey::from_str("5PjJyuCBgUNc5U8CEHRLUZZx4nCYVAZPB8mvozcB6VkQ").unwrap(),
    Pubkey::from_str("5PjGE68Zwybq3FV5F4idGsszhHSbnPBTdqbY5Vd52Qxc").unwrap(),
    Pubkey::from_str("5PjG85mRBau6CY5vQPSPKc8t3ugY11yJtCy2u9Wex4VT").unwrap(),
    Pubkey::from_str("5PjNkxszqYkpaYJAQmTLLRTrMiEZaUuzdJjtMaoCFWUJ").unwrap(),
    Pubkey::from_str("5PjNzvFvERne7c9SNPUcoyxBhGs6cGLUPx8iBNs9tzkP").unwrap(),
    Pubkey::from_str("5PjHWBn7n6YxmgM2E7Tqh92uJRY9miCFuHA3fcGZ7t25").unwrap(),
    Pubkey::from_str("5PjPU26ksAQVrNGFANshh6Q19QURnuQ6aiYNzimx1jvS").unwrap(),
    Pubkey::from_str("5PjHWDkjJakdZq8xtHQ5Ty4tRuPNYt6212FMqRFDKAEK").unwrap(),
    Pubkey::from_str("5PjNuJtoZcFuViDHfrh6BbeXiEQeB3j3zTvP8xXAAkhK").unwrap(),
    Pubkey::from_str("5PjLmCzsWMoRpYcDb1mNCN1D4GAu3p7dX3SDPgadrhLC").unwrap(),
    Pubkey::from_str("5PjNrNmpTg7NRXmsNTypvm3i71VXxpTPAvYvxvMeCWfH").unwrap(),
    Pubkey::from_str("5PjKxVxhp83KhYUhG1J7rjmBFUYbiK9MxuqC1H555unn").unwrap(),
    Pubkey::from_str("5PjMfp3iPatvQbmopjzWNKaHWqvUscdzKVSwaGTxdXjm").unwrap(),
    Pubkey::from_str("5PjM3eeQ16sa9mAxzUhxr3b6sMqzpZscixoUeK5ahwNL").unwrap(),
    Pubkey::from_str("5PjLEQhLU5kXERWeckm5vn6tVRzrkMNkTMEYxJgGaDZ4").unwrap(),
    Pubkey::from_str("5PjNqgKKyyQAsSq2FCi1pu764pUUHn5oMU9wj7mg3nr8").unwrap(),
    Pubkey::from_str("5PjJfuKcjdQ7szFTuQEJDia3pJPG3g3FK1nQixj9RG22").unwrap(),
    Pubkey::from_str("5PjJn2ZsARCP1qncJTdojjyYgap3SQzjxBMAS5GqSpBG").unwrap(),
    Pubkey::from_str("5PjNwBSRpxANuAEbE1dZo2DT6u8YpN6zp2jUxrQjLoJx").unwrap(),
    Pubkey::from_str("5PjLn73H8nUQvFVYx4Zdx9YZHpeL92QWG7X7oebkuy2v").unwrap(),
    Pubkey::from_str("5PjK7KEo7MUvZesrAaNgMHEN9Go66nBx3EjSUteijjUL").unwrap(),
    Pubkey::from_str("5PjHaakHZFGfWULb9CdzpeJEkog7FXKf5VyQt1GLHCRG").unwrap(),
    Pubkey::from_str("5PjP1Ej5q1i7GRSn9eDe9MFyyfwwSDtczaRE9hYuEhMC").unwrap(),
    Pubkey::from_str("5PjGTDZYQCMHVwrKnuF9o7WFi1uQvudgRRwo1mfdeQKm").unwrap(),
    Pubkey::from_str("5PjL5jBmD35y69gkctoxxV9K7XQ9p1p4n485Ju7zZUZp").unwrap(),
    Pubkey::from_str("5PjH5zu3RJUTKKks3vVpgJeB32d8Ed5PJavZS84xJQyE").unwrap(),
    Pubkey::from_str("5PjHcVfyjzzDVqW7sb9G7mSA9NdpZJk5GkBPN64rNTkS").unwrap(),
    Pubkey::from_str("5PjHUH3GNtbhYTii8NrZ1mAkh3DB8jB9QESfMruYLbAh").unwrap(),
    Pubkey::from_str("5PjNnyYoo8PQDTxoGXUkqTevPD8qbiutUh3P7LuvT4uj").unwrap(),
    Pubkey::from_str("5PjGQN996dcixoqqsMCGJ8D4Fcd5cxfm5LqAFasDp4NV").unwrap(),
    Pubkey::from_str("6V3dURW8Hn7Vsw4teZ3mvqk9N8GotA1mMkoeHkyjYmgF").unwrap(),
    Pubkey::from_str("5PjHr9wCUoxjTcp4rPwrYEfWa7XGP8v86rtgWmyqSrWi").unwrap(),
    Pubkey::from_str("5PjLaHr1HqrzhUB2eRWyYetzLJFnobY4UTip8ReqaamW").unwrap(),
    Pubkey::from_str("CX83CaJ29cJLHVGEEnwJGSJvwtsRjeneKEsysvYiR6SA").unwrap(),
    Pubkey::from_str("5PjMF9zB3BsANzbF5CtEHqMq8ULV1FVi73AgmDCjXWzi").unwrap(),];    
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
    let no_snapshot_arg = Arg::with_name("no_snapshot")
        .long("no-snapshot")
        .takes_value(false)
        .help("Do not start from a local snapshot if present");
    let no_bpf_jit_arg = Arg::with_name("no_bpf_jit")
        .long("no-bpf-jit")
        .takes_value(false)
        .help("Disable the just-in-time compiler and instead use the interpreter for BP");
    let no_accounts_db_caching_arg = Arg::with_name("no_accounts_db_caching")
        .long("no-accounts-db-caching")
        .takes_value(false)
        .help("Disables accounts-db caching");
    let accounts_index_bins = Arg::with_name("accounts_index_bins")
        .long("accounts-index-bins")
        .value_name("BINS")
        .validator(is_pow2)
        .takes_value(true)
        .help("Number of bins to divide the accounts index into");
    let accounts_index_limit = Arg::with_name("accounts_index_memory_limit_mb")
        .long("accounts-index-memory-limit-mb")
        .value_name("MEGABYTES")
        .validator(is_parsable::<usize>)
        .takes_value(true)
        .help("How much memory the accounts index can consume. If this is exceeded, some account index entries will be stored on disk. If missing, the entire index is stored in memory.");
    let disable_disk_index = Arg::with_name("disable_accounts_disk_index")
        .long("disable-accounts-disk-index")
        .help("Disable the disk-based accounts index if it is enabled by default.")
        .conflicts_with("accounts_index_memory_limit_mb");
    let accountsdb_skip_shrink = Arg::with_name("accounts_db_skip_shrink")
        .long("accounts-db-skip-shrink")
        .help(
            "Enables faster starting of ledger-tool by skipping shrink. \
                      This option is for use during testing.",
        );
    let accounts_filler_count = Arg::with_name("accounts_filler_count")
        .long("accounts-filler-count")
        .value_name("COUNT")
        .validator(is_parsable::<usize>)
        .takes_value(true)
        .default_value("0")
        .help("How many accounts to add to stress the system. Accounts are ignored in operations related to correctness.");
    let accounts_filler_size = Arg::with_name("accounts_filler_size")
        .long("accounts-filler-size")
        .value_name("BYTES")
        .validator(is_parsable::<usize>)
        .takes_value(true)
        .default_value("0")
        .requires("accounts_filler_count")
        .help("Size per filler account in bytes.");
    let account_paths_arg = Arg::with_name("account_paths")
        .long("accounts")
        .value_name("PATHS")
        .takes_value(true)
        .help("Comma separated persistent accounts location");
    let accounts_index_path_arg = Arg::with_name("accounts_index_path")
        .long("accounts-index-path")
        .value_name("PATH")
        .takes_value(true)
        .multiple(true)
        .help(
            "Persistent accounts-index location. \
             May be specified multiple times. \
             [default: [ledger]/accounts_index]",
        );
    let accounts_db_test_hash_calculation_arg = Arg::with_name("accounts_db_test_hash_calculation")
        .long("accounts-db-test-hash-calculation")
        .help("Enable hash calculation test");
    let halt_at_slot_arg = Arg::with_name("halt_at_slot")
        .long("halt-at-slot")
        .value_name("SLOT")
        .validator(is_slot)
        .takes_value(true)
        .help("Halt processing at the given slot");
    let no_os_memory_stats_reporting_arg = Arg::with_name("no_os_memory_stats_reporting")
        .long("no-os-memory-stats-reporting")
        .help("Disable reporting of OS memory statistics.");
    let skip_rewrites_arg = Arg::with_name("accounts_db_skip_rewrites")
        .long("accounts-db-skip-rewrites")
        .help(
            "Accounts that are rent exempt and have no changes are not rewritten. \
                  This produces snapshots that older versions cannot read.",
        )
        .hidden(true);
    let verify_index_arg = Arg::with_name("verify_accounts_index")
        .long("verify-accounts-index")
        .takes_value(false)
        .help("For debugging and tests on accounts index.");
    let limit_load_slot_count_from_snapshot_arg = Arg::with_name("limit_load_slot_count_from_snapshot")
        .long("limit-load-slot-count-from-snapshot")
        .value_name("SLOT")
        .validator(is_slot)
        .takes_value(true)
        .help("For debugging and profiling with large snapshots, artificially limit how many slots are loaded from a snapshot.");
    let hard_forks_arg = Arg::with_name("hard_forks")
        .long("hard-fork")
        .value_name("SLOT")
        .validator(is_slot)
        .multiple(true)
        .takes_value(true)
        .help("Add a hard fork at this slot");
    let allow_dead_slots_arg = Arg::with_name("allow_dead_slots")
        .long("allow-dead-slots")
        .takes_value(false)
        .help("Output dead slots as well");
    let default_genesis_archive_unpacked_size = MAX_GENESIS_ARCHIVE_UNPACKED_SIZE.to_string();
    let max_genesis_archive_unpacked_size_arg = Arg::with_name("max_genesis_archive_unpacked_size")
        .long("max-genesis-archive-unpacked-size")
        .value_name("NUMBER")
        .takes_value(true)
        .default_value(&default_genesis_archive_unpacked_size)
        .help("maximum total uncompressed size of unpacked genesis archive");
    let hashes_per_tick = Arg::with_name("hashes_per_tick")
        .long("hashes-per-tick")
        .value_name("NUM_HASHES|\"sleep\"")
        .takes_value(true)
        .help(
            "How many PoH hashes to roll before emitting the next tick. \
             If \"sleep\", for development \
             sleep for the target tick duration instead of hashing",
        );
    let snapshot_version_arg = Arg::with_name("snapshot_version")
        .long("snapshot-version")
        .value_name("SNAPSHOT_VERSION")
        .validator(is_parsable::<SnapshotVersion>)
        .takes_value(true)
        .default_value(SnapshotVersion::default().into())
        .help("Output snapshot version");

    let default_max_full_snapshot_archives_to_retain =
        &DEFAULT_MAX_FULL_SNAPSHOT_ARCHIVES_TO_RETAIN.to_string();
    let maximum_full_snapshot_archives_to_retain = Arg::with_name(
        "maximum_full_snapshots_to_retain",
    )
    .long("maximum-full-snapshots-to-retain")
    .alias("maximum-snapshots-to-retain")
    .value_name("NUMBER")
    .takes_value(true)
    .default_value(default_max_full_snapshot_archives_to_retain)
    .help(
        "The maximum number of full snapshot archives to hold on to when purging older snapshots.",
    );

    let default_max_incremental_snapshot_archives_to_retain =
        &DEFAULT_MAX_INCREMENTAL_SNAPSHOT_ARCHIVES_TO_RETAIN.to_string();
    let maximum_incremental_snapshot_archives_to_retain = Arg::with_name(
        "maximum_incremental_snapshots_to_retain",
    )
    .long("maximum-incremental-snapshots-to-retain")
    .value_name("NUMBER")
    .takes_value(true)
    .default_value(default_max_incremental_snapshot_archives_to_retain)
    .help("The maximum number of incremental snapshot archives to hold on to when purging older snapshots.");

    let rent = Rent::default();
    let default_bootstrap_validator_lamports = &sol_to_lamports(500.0)
        .max(VoteState::get_rent_exempt_reserve(&rent))
        .to_string();
    let default_bootstrap_validator_stake_lamports = &sol_to_lamports(0.5)
        .max(rent.minimum_balance(StakeState::size_of()))
        .to_string();

    let matches = App::new(crate_name!())
        .about(crate_description!())
        .version(solana_version::version!())
        .setting(AppSettings::InferSubcommands)
        .setting(AppSettings::SubcommandRequiredElseHelp)
        .setting(AppSettings::VersionlessSubcommands)
        .arg(
            Arg::with_name("ledger_path")
                .short("l")
                .long("ledger")
                .value_name("DIR")
                .takes_value(true)
                .global(true)
                .default_value("ledger")
                .help("Use DIR as ledger location"),
        )
        .arg(
            Arg::with_name("wal_recovery_mode")
                .long("wal-recovery-mode")
                .value_name("MODE")
                .takes_value(true)
                .global(true)
                .possible_values(&[
                    "tolerate_corrupted_tail_records",
                    "absolute_consistency",
                    "point_in_time",
                    "skip_any_corrupted_record"])
                .help(
                    "Mode to recovery the ledger db write ahead log"
                ),
        )
        .arg(
            Arg::with_name("snapshot_archive_path")
                .long("snapshot-archive-path")
                .value_name("DIR")
                .takes_value(true)
                .global(true)
                .help("Use DIR for ledger location"),
        )
        .arg(
            Arg::with_name("output_format")
                .long("output")
                .value_name("FORMAT")
                .global(true)
                .takes_value(true)
                .possible_values(&["json", "json-compact"])
                .help("Return information in specified output format, \
                       currently only available for bigtable subcommands"),
        )
        .arg(
            Arg::with_name("verbose")
                .short("v")
                .long("verbose")
                .global(true)
                .multiple(true)
                .takes_value(false)
                .help("Show additional information where supported"),
        )
        .bigtable_subcommand()
        .subcommand(
            SubCommand::with_name("print")
            .about("Print the ledger")
            .arg(&starting_slot_arg)
            .arg(&allow_dead_slots_arg)
            .arg(&ending_slot_arg)
            .arg(
                Arg::with_name("num_slots")
                    .long("num-slots")
                    .value_name("SLOT")
                    .validator(is_slot)
                    .takes_value(true)
                    .help("Number of slots to print"),
            )
            .arg(
                Arg::with_name("only_rooted")
                    .long("only-rooted")
                    .takes_value(false)
                    .help("Only print root slots"),
            )
        )
        .subcommand(
            SubCommand::with_name("copy")
            .about("Copy the ledger")
            .arg(&starting_slot_arg)
            .arg(&ending_slot_arg)
            .arg(
                Arg::with_name("target_db")
                    .long("target-db")
                    .value_name("DIR")
                    .takes_value(true)
                    .help("Target db"),
            )
        )
        .subcommand(
            SubCommand::with_name("slot")
            .about("Print the contents of one or more slots")
            .arg(
                Arg::with_name("slots")
                    .index(1)
                    .value_name("SLOTS")
                    .validator(is_slot)
                    .takes_value(true)
                    .multiple(true)
                    .required(true)
                    .help("Slots to print"),
            )
            .arg(&allow_dead_slots_arg)
        )
        .subcommand(
            SubCommand::with_name("dead-slots")
            .arg(&starting_slot_arg)
            .about("Print all the dead slots in the ledger")
        )
        .subcommand(
            SubCommand::with_name("duplicate-slots")
            .arg(&starting_slot_arg)
            .about("Print all the duplicate slots in the ledger")
        )
        .subcommand(
            SubCommand::with_name("set-dead-slot")
            .about("Mark one or more slots dead")
            .arg(
                Arg::with_name("slots")
                    .index(1)
                    .value_name("SLOTS")
                    .validator(is_slot)
                    .takes_value(true)
                    .multiple(true)
                    .required(true)
                    .help("Slots to mark dead"),
            )
        )
        .subcommand(
            SubCommand::with_name("remove-dead-slot")
            .about("Remove the dead flag for a slot")
            .arg(
                Arg::with_name("slots")
                    .index(1)
                    .value_name("SLOTS")
                    .validator(is_slot)
                    .takes_value(true)
                    .multiple(true)
                    .required(true)
                    .help("Slots to mark as not dead"),
            )
        )
        .subcommand(
            SubCommand::with_name("genesis")
            .about("Prints the ledger's genesis config")
            .arg(&max_genesis_archive_unpacked_size_arg)
            .arg(
                Arg::with_name("accounts")
                    .long("accounts")
                    .takes_value(false)
                    .help("Print the ledger's genesis accounts"),
            )
            .arg(
                Arg::with_name("no_account_data")
                    .long("no-account-data")
                    .takes_value(false)
                    .requires("accounts")
                    .help("Do not print account data when printing account contents."),
            )
        )
        .subcommand(
            SubCommand::with_name("genesis-hash")
            .about("Prints the ledger's genesis hash")
            .arg(&max_genesis_archive_unpacked_size_arg)
        )
        .subcommand(
            SubCommand::with_name("parse_full_frozen")
            .about("Parses log for information about critical events about \
                    ancestors of the given `ending_slot`")
            .arg(&starting_slot_arg)
            .arg(&ending_slot_arg)
            .arg(
                Arg::with_name("log_path")
                    .long("log-path")
                    .value_name("PATH")
                    .takes_value(true)
                    .help("path to log file to parse"),
            )
        )
        .subcommand(
            SubCommand::with_name("modify-genesis")
            .about("Modifies genesis parameters")
            .arg(&max_genesis_archive_unpacked_size_arg)
            .arg(&hashes_per_tick)
            .arg(
                Arg::with_name("cluster_type")
                    .long("cluster-type")
                    .possible_values(&ClusterType::STRINGS)
                    .takes_value(true)
                    .help(
                        "Selects the features that will be enabled for the cluster"
                    ),
            )
            .arg(
                Arg::with_name("output_directory")
                    .index(1)
                    .value_name("DIR")
                    .takes_value(true)
                    .help("Output directory for the modified genesis config"),
            )
        )
        .subcommand(
            SubCommand::with_name("shred-version")
            .about("Prints the ledger's shred hash")
            .arg(&hard_forks_arg)
            .arg(&max_genesis_archive_unpacked_size_arg)
        )
        .subcommand(
            SubCommand::with_name("shred-meta")
            .about("Prints raw shred metadata")
            .arg(&starting_slot_arg)
            .arg(&ending_slot_arg)
        )
        .subcommand(
            SubCommand::with_name("bank-hash")
            .about("Prints the hash of the working bank after reading the ledger")
            .arg(&max_genesis_archive_unpacked_size_arg)
        )
        .subcommand(
            SubCommand::with_name("bounds")
            .about("Print lowest and highest non-empty slots. \
                    Note that there may be empty slots within the bounds")
            .arg(
                Arg::with_name("all")
                    .long("all")
                    .takes_value(false)
                    .required(false)
                    .help("Additionally print all the non-empty slots within the bounds"),
            )
        ).subcommand(
            SubCommand::with_name("json")
            .about("Print the ledger in JSON format")
            .arg(&starting_slot_arg)
            .arg(&allow_dead_slots_arg)
        )
        .subcommand(
            SubCommand::with_name("verify")
            .about("Verify the ledger")
            .arg(&no_snapshot_arg)
            .arg(&account_paths_arg)
            .arg(&accounts_index_path_arg)
            .arg(&halt_at_slot_arg)
            .arg(&limit_load_slot_count_from_snapshot_arg)
            .arg(&accounts_index_bins)
            .arg(&accounts_index_limit)
            .arg(&disable_disk_index)
            .arg(&accountsdb_skip_shrink)
            .arg(&accounts_filler_count)
            .arg(&accounts_filler_size)
            .arg(&verify_index_arg)
            .arg(&skip_rewrites_arg)
            .arg(&hard_forks_arg)
            .arg(&no_accounts_db_caching_arg)
            .arg(&accounts_db_test_hash_calculation_arg)
            .arg(&no_os_memory_stats_reporting_arg)
            .arg(&no_bpf_jit_arg)
            .arg(&allow_dead_slots_arg)
            .arg(&max_genesis_archive_unpacked_size_arg)
            .arg(
                Arg::with_name("skip_poh_verify")
                    .long("skip-poh-verify")
                    .takes_value(false)
                    .help("Skip ledger PoH verification"),
            )
            .arg(
                Arg::with_name("print_accounts_stats")
                    .long("print-accounts-stats")
                    .takes_value(false)
                    .help("After verifying the ledger, print some information about the account stores"),
            )
        ).subcommand(
            SubCommand::with_name("graph")
            .about("Create a Graphviz rendering of the ledger")
            .arg(&no_snapshot_arg)
            .arg(&account_paths_arg)
            .arg(&halt_at_slot_arg)
            .arg(&hard_forks_arg)
            .arg(&max_genesis_archive_unpacked_size_arg)
            .arg(
                Arg::with_name("include_all_votes")
                    .long("include-all-votes")
                    .help("Include all votes in the graph"),
            )
            .arg(
                Arg::with_name("graph_filename")
                    .index(1)
                    .value_name("FILENAME")
                    .takes_value(true)
                    .help("Output file"),
            )
        ).subcommand(
            SubCommand::with_name("create-snapshot")
            .about("Create a new ledger snapshot")
            .arg(&no_snapshot_arg)
            .arg(&account_paths_arg)
            .arg(&hard_forks_arg)
            .arg(&max_genesis_archive_unpacked_size_arg)
            .arg(&snapshot_version_arg)
            .arg(&maximum_full_snapshot_archives_to_retain)
            .arg(&maximum_incremental_snapshot_archives_to_retain)
            .arg(
                Arg::with_name("snapshot_slot")
                    .index(1)
                    .value_name("SLOT")
                    .validator(|value| {
                        if value.parse::<Slot>().is_ok()
                            || value == "ROOT"
                        {
                            Ok(())
                        } else {
                            Err(format!(
                                "Unable to parse as a number or the keyword ROOT, provided: {}",
                                value
                            ))
                        }
                    })
                    .takes_value(true)
                    .help("Slot at which to create the snapshot; accepts keyword ROOT for the highest root"),
            )
            .arg(
                Arg::with_name("output_directory")
                    .index(2)
                    .value_name("DIR")
                    .takes_value(true)
                    .help("Output directory for the snapshot [default: --snapshot-archive-path if present else --ledger directory]"),
            )
            .arg(
                Arg::with_name("warp_slot")
                    .required(false)
                    .long("warp-slot")
                    .takes_value(true)
                    .value_name("WARP_SLOT")
                    .validator(is_slot)
                    .help("After loading the snapshot slot warp the ledger to WARP_SLOT, \
                           which could be a slot in a galaxy far far away"),
            )
            .arg(
                Arg::with_name("faucet_lamports")
                    .short("t")
                    .long("faucet-lamports")
                    .value_name("LAMPORTS")
                    .takes_value(true)
                    .requires("faucet_pubkey")
                    .help("Number of lamports to assign to the faucet"),
            )
            .arg(
                Arg::with_name("faucet_pubkey")
                    .short("m")
                    .long("faucet-pubkey")
                    .value_name("PUBKEY")
                    .takes_value(true)
                    .validator(is_pubkey_or_keypair)
                    .requires("faucet_lamports")
                    .help("Path to file containing the faucet's pubkey"),
            )
            .arg(
                Arg::with_name("bootstrap_validator")
                    .short("b")
                    .long("bootstrap-validator")
                    .value_name("IDENTITY_PUBKEY VOTE_PUBKEY STAKE_PUBKEY")
                    .takes_value(true)
                    .validator(is_pubkey_or_keypair)
                    .number_of_values(3)
                    .multiple(true)
                    .help("The bootstrap validator's identity, vote and stake pubkeys"),
            )
            .arg(
                Arg::with_name("bootstrap_stake_authorized_pubkey")
                    .long("bootstrap-stake-authorized-pubkey")
                    .value_name("BOOTSTRAP STAKE AUTHORIZED PUBKEY")
                    .takes_value(true)
                    .validator(is_pubkey_or_keypair)
                    .help(
                        "Path to file containing the pubkey authorized to manage the bootstrap \
                         validator's stake [default: --bootstrap-validator IDENTITY_PUBKEY]",
                    ),
            )
            .arg(
                Arg::with_name("bootstrap_validator_lamports")
                    .long("bootstrap-validator-lamports")
                    .value_name("LAMPORTS")
                    .takes_value(true)
                    .default_value(default_bootstrap_validator_lamports)
                    .help("Number of lamports to assign to the bootstrap validator"),
            )
            .arg(
                Arg::with_name("bootstrap_validator_stake_lamports")
                    .long("bootstrap-validator-stake-lamports")
                    .value_name("LAMPORTS")
                    .takes_value(true)
                    .default_value(default_bootstrap_validator_stake_lamports)
                    .help("Number of lamports to assign to the bootstrap validator's stake account"),
            )
            .arg(
                Arg::with_name("rent_burn_percentage")
                    .long("rent-burn-percentage")
                    .value_name("NUMBER")
                    .takes_value(true)
                    .help("Adjust percentage of collected rent to burn")
                    .validator(is_valid_percentage),
            )
            .arg(&hashes_per_tick)
            .arg(
                Arg::with_name("accounts_to_remove")
                    .required(false)
                    .long("remove-account")
                    .takes_value(true)
                    .value_name("PUBKEY")
                    .validator(is_pubkey)
                    .multiple(true)
                    .help("List of accounts to remove while creating the snapshot"),
            )
            .arg(
                Arg::with_name("vote_accounts_to_destake")
                    .required(false)
                    .long("destake-vote-account")
                    .takes_value(true)
                    .value_name("PUBKEY")
                    .validator(is_pubkey)
                    .multiple(true)
                    .help("List of validator vote accounts to destake")
            )
            .arg(
                Arg::with_name("remove_stake_accounts")
                    .required(false)
                    .long("remove-stake-accounts")
                    .takes_value(false)
                    .help("Remove all existing stake accounts from the new snapshot")
            )
            .arg(
                Arg::with_name("incremental")
                    .long("incremental")
                    .takes_value(false)
                    .help("Create an incremental snapshot instead of a full snapshot. This requires \
                          that the ledger is loaded from a full snapshot, which will be used as the \
                          base for the incremental snapshot.")
                    .conflicts_with("no_snapshot")
            )
        ).subcommand(
            SubCommand::with_name("accounts")
            .about("Print account stats and contents after processing the ledger")
            .arg(&no_snapshot_arg)
            .arg(&account_paths_arg)
            .arg(&halt_at_slot_arg)
            .arg(&hard_forks_arg)
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
            .arg(&max_genesis_archive_unpacked_size_arg)
        ).subcommand(
            SubCommand::with_name("capitalization")
            .about("Print capitalization (aka, total supply) while checksumming it")
            .arg(&no_snapshot_arg)
            .arg(&account_paths_arg)
            .arg(&halt_at_slot_arg)
            .arg(&hard_forks_arg)
            .arg(&max_genesis_archive_unpacked_size_arg)
            .arg(
                Arg::with_name("warp_epoch")
                    .required(false)
                    .long("warp-epoch")
                    .takes_value(true)
                    .value_name("WARP_EPOCH")
                    .help("After loading the snapshot warp the ledger to WARP_EPOCH, \
                           which could be an epoch in a galaxy far far away"),
            )
            .arg(
                Arg::with_name("inflation")
                    .required(false)
                    .long("inflation")
                    .takes_value(true)
                    .possible_values(&["pico", "full", "none"])
                    .help("Overwrite inflation when warping"),
            )
            .arg(
                Arg::with_name("enable_credits_auto_rewind")
                    .required(false)
                    .long("enable-credits-auto-rewind")
                    .takes_value(false)
                    .help("Enable credits auto rewind"),
            )
            .arg(
                Arg::with_name("recalculate_capitalization")
                    .required(false)
                    .long("recalculate-capitalization")
                    .takes_value(false)
                    .help("Recalculate capitalization before warping; circumvents \
                          bank's out-of-sync capitalization"),
            )
            .arg(
                Arg::with_name("csv_filename")
                    .long("csv-filename")
                    .value_name("FILENAME")
                    .takes_value(true)
                    .help("Output file in the csv format"),
            )
        ).subcommand(
            SubCommand::with_name("purge")
            .about("Delete a range of slots from the ledger")
            .arg(
                Arg::with_name("start_slot")
                    .index(1)
                    .value_name("SLOT")
                    .takes_value(true)
                    .required(true)
                    .help("Start slot to purge from (inclusive)"),
            )
            .arg(
                Arg::with_name("end_slot")
                    .index(2)
                    .value_name("SLOT")
                    .help("Ending slot to stop purging (inclusive) \
                           [default: the highest slot in the ledger]"),
            )
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
                    .help("Skip ledger compaction after purge")
            )
            .arg(
                Arg::with_name("dead_slots_only")
                    .long("dead-slots-only")
                    .required(false)
                    .takes_value(false)
                    .help("Limit purging to dead slots only")
            )
        )
        .subcommand(
            SubCommand::with_name("list-roots")
            .about("Output up to last <num-roots> root hashes and their \
                    heights starting at the given block height")
            .arg(
                Arg::with_name("max_height")
                    .long("max-height")
                    .value_name("NUM")
                    .takes_value(true)
                    .help("Maximum block height")
            )
            .arg(
                Arg::with_name("start_root")
                    .long("start-root")
                    .value_name("NUM")
                    .takes_value(true)
                    .help("First root to start searching from")
            )
            .arg(
                Arg::with_name("slot_list")
                    .long("slot-list")
                    .value_name("FILENAME")
                    .required(false)
                    .takes_value(true)
                    .help("The location of the output YAML file. A list of \
                           rollback slot heights and hashes will be written to the file")
            )
            .arg(
                Arg::with_name("num_roots")
                    .long("num-roots")
                    .value_name("NUM")
                    .takes_value(true)
                    .default_value(DEFAULT_ROOT_COUNT)
                    .required(false)
                    .help("Number of roots in the output"),
            )
        )
        .subcommand(
            SubCommand::with_name("repair-roots")
                .about("Traverses the AncestorIterator backward from a last known root \
                        to restore missing roots to the Root column")
                .arg(
                    Arg::with_name("start_root")
                        .long("before")
                        .value_name("NUM")
                        .takes_value(true)
                        .help("First good root after the range to repair")
                )
                .arg(
                    Arg::with_name("end_root")
                        .long("until")
                        .value_name("NUM")
                        .takes_value(true)
                        .help("Last slot to check for root repair")
                )
                .arg(
                    Arg::with_name("max_slots")
                        .long("repair-limit")
                        .value_name("NUM")
                        .takes_value(true)
                        .default_value(DEFAULT_MAX_SLOTS_ROOT_REPAIR)
                        .required(true)
                        .help("Override the maximum number of slots to check for root repair")
                )
        )
        .subcommand(
            SubCommand::with_name("analyze-storage")
                .about("Output statistics in JSON format about \
                        all column families in the ledger rocksdb")
        )
        .subcommand(
            SubCommand::with_name("compute-slot-cost")
            .about("runs cost_model over the block at the given slots, \
                   computes how expensive a block was based on cost_model")
            .arg(
                Arg::with_name("slots")
                    .index(1)
                    .value_name("SLOTS")
                    .validator(is_slot)
                    .multiple(true)
                    .takes_value(true)
                    .help("Slots that their blocks are computed for cost, default to all slots in ledger"),
            )
        )
        .get_matches();

    info!("{} {}", crate_name!(), solana_version::version!());

    let ledger_path = parse_ledger_path(&matches, "ledger_path");

    let snapshot_archive_path = value_t!(matches, "snapshot_archive_path", String)
        .ok()
        .map(PathBuf::from);

    let wal_recovery_mode = matches
        .value_of("wal_recovery_mode")
        .map(BlockstoreRecoveryMode::from);
    let verbose_level = matches.occurrences_of("verbose");

    if let ("bigtable", Some(arg_matches)) = matches.subcommand() {
        bigtable_process_command(&ledger_path, arg_matches)
    } else {
        let ledger_path = canonicalize_ledger_path(&ledger_path);

        match matches.subcommand() {
            ("print", Some(arg_matches)) => {
                let starting_slot = value_t_or_exit!(arg_matches, "starting_slot", Slot);
                let ending_slot = value_t!(arg_matches, "ending_slot", Slot).unwrap_or(Slot::MAX);
                let num_slots = value_t!(arg_matches, "num_slots", Slot).ok();
                let allow_dead_slots = arg_matches.is_present("allow_dead_slots");
                let only_rooted = arg_matches.is_present("only_rooted");
                output_ledger(
                    open_blockstore(&ledger_path, AccessType::Secondary, wal_recovery_mode),
                    starting_slot,
                    ending_slot,
                    allow_dead_slots,
                    LedgerOutputMethod::Print,
                    num_slots,
                    verbose_level,
                    only_rooted,
                );
            }
            ("copy", Some(arg_matches)) => {
                let starting_slot = value_t_or_exit!(arg_matches, "starting_slot", Slot);
                let ending_slot = value_t_or_exit!(arg_matches, "ending_slot", Slot);
                let target_db = PathBuf::from(value_t_or_exit!(arg_matches, "target_db", String));
                let source = open_blockstore(&ledger_path, AccessType::Secondary, None);
                let target = open_blockstore(&target_db, AccessType::Primary, None);
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
            ("genesis", Some(arg_matches)) => {
                let genesis_config = open_genesis_config_by(&ledger_path, arg_matches);
                let print_accouunts = arg_matches.is_present("accounts");
                if print_accouunts {
                    let print_account_data = !arg_matches.is_present("no_account_data");
                    for (pubkey, account) in genesis_config.accounts {
                        output_account(
                            &pubkey,
                            &AccountSharedData::from(account),
                            None,
                            print_account_data,
                        );
                    }
                } else {
                    println!("{}", genesis_config);
                }
            }
            ("genesis-hash", Some(arg_matches)) => {
                println!(
                    "{}",
                    open_genesis_config_by(&ledger_path, arg_matches).hash()
                );
            }
            ("modify-genesis", Some(arg_matches)) => {
                let mut genesis_config = open_genesis_config_by(&ledger_path, arg_matches);
                let output_directory =
                    PathBuf::from(arg_matches.value_of("output_directory").unwrap());

                if let Some(cluster_type) = cluster_type_of(arg_matches, "cluster_type") {
                    genesis_config.cluster_type = cluster_type;
                }

                if let Some(hashes_per_tick) = arg_matches.value_of("hashes_per_tick") {
                    genesis_config.poh_config.hashes_per_tick = match hashes_per_tick {
                        // Note: Unlike `solana-genesis`, "auto" is not supported here.
                        "sleep" => None,
                        _ => Some(value_t_or_exit!(arg_matches, "hashes_per_tick", u64)),
                    }
                }

                create_new_ledger(
                    &output_directory,
                    &genesis_config,
                    solana_runtime::hardened_unpack::MAX_GENESIS_ARCHIVE_UNPACKED_SIZE,
                    LedgerColumnOptions::default(),
                )
                .unwrap_or_else(|err| {
                    eprintln!("Failed to write genesis config: {:?}", err);
                    exit(1);
                });

                println!("{}", open_genesis_config_by(&output_directory, arg_matches));
            }
            ("shred-version", Some(arg_matches)) => {
                let process_options = ProcessOptions {
                    new_hard_forks: hardforks_of(arg_matches, "hard_forks"),
                    halt_at_slot: Some(0),
                    poh_verify: false,
                    ..ProcessOptions::default()
                };
                let genesis_config = open_genesis_config_by(&ledger_path, arg_matches);
                let blockstore =
                    open_blockstore(&ledger_path, AccessType::Secondary, wal_recovery_mode);
                match load_bank_forks(
                    arg_matches,
                    &genesis_config,
                    &blockstore,
                    process_options,
                    snapshot_archive_path,
                ) {
                    Ok((bank_forks, ..)) => {
                        println!(
                            "{}",
                            compute_shred_version(
                                &genesis_config.hash(),
                                Some(
                                    &bank_forks
                                        .read()
                                        .unwrap()
                                        .working_bank()
                                        .hard_forks()
                                        .read()
                                        .unwrap()
                                )
                            )
                        );
                    }
                    Err(err) => {
                        eprintln!("Failed to load ledger: {:?}", err);
                        exit(1);
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
                let ledger = open_blockstore(&ledger_path, AccessType::Secondary, None);
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
            ("bank-hash", Some(arg_matches)) => {
                let process_options = ProcessOptions {
                    new_hard_forks: hardforks_of(arg_matches, "hard_forks"),
                    halt_at_slot: Some(0),
                    poh_verify: false,
                    ..ProcessOptions::default()
                };
                let genesis_config = open_genesis_config_by(&ledger_path, arg_matches);
                let blockstore =
                    open_blockstore(&ledger_path, AccessType::Secondary, wal_recovery_mode);
                match load_bank_forks(
                    arg_matches,
                    &genesis_config,
                    &blockstore,
                    process_options,
                    snapshot_archive_path,
                ) {
                    Ok((bank_forks, ..)) => {
                        println!("{}", &bank_forks.read().unwrap().working_bank().hash());
                    }
                    Err(err) => {
                        eprintln!("Failed to load ledger: {:?}", err);
                        exit(1);
                    }
                }
            }
            ("slot", Some(arg_matches)) => {
                let slots = values_t_or_exit!(arg_matches, "slots", Slot);
                let allow_dead_slots = arg_matches.is_present("allow_dead_slots");
                let blockstore =
                    open_blockstore(&ledger_path, AccessType::Secondary, wal_recovery_mode);
                for slot in slots {
                    println!("Slot {}", slot);
                    if let Err(err) = output_slot(
                        &blockstore,
                        slot,
                        allow_dead_slots,
                        &LedgerOutputMethod::Print,
                        verbose_level,
                    ) {
                        eprintln!("{}", err);
                    }
                }
            }
            ("json", Some(arg_matches)) => {
                let starting_slot = value_t_or_exit!(arg_matches, "starting_slot", Slot);
                let allow_dead_slots = arg_matches.is_present("allow_dead_slots");
                output_ledger(
                    open_blockstore(&ledger_path, AccessType::Secondary, wal_recovery_mode),
                    starting_slot,
                    Slot::MAX,
                    allow_dead_slots,
                    LedgerOutputMethod::Json,
                    None,
                    std::u64::MAX,
                    true,
                );
            }
            ("dead-slots", Some(arg_matches)) => {
                let blockstore =
                    open_blockstore(&ledger_path, AccessType::Secondary, wal_recovery_mode);
                let starting_slot = value_t_or_exit!(arg_matches, "starting_slot", Slot);
                for slot in blockstore.dead_slots_iterator(starting_slot).unwrap() {
                    println!("{}", slot);
                }
            }
            ("duplicate-slots", Some(arg_matches)) => {
                let blockstore =
                    open_blockstore(&ledger_path, AccessType::Secondary, wal_recovery_mode);
                let starting_slot = value_t_or_exit!(arg_matches, "starting_slot", Slot);
                for slot in blockstore.duplicate_slots_iterator(starting_slot).unwrap() {
                    println!("{}", slot);
                }
            }
            ("set-dead-slot", Some(arg_matches)) => {
                let slots = values_t_or_exit!(arg_matches, "slots", Slot);
                let blockstore =
                    open_blockstore(&ledger_path, AccessType::Primary, wal_recovery_mode);
                for slot in slots {
                    match blockstore.set_dead_slot(slot) {
                        Ok(_) => println!("Slot {} dead", slot),
                        Err(err) => eprintln!("Failed to set slot {} dead slot: {:?}", slot, err),
                    }
                }
            }
            ("remove-dead-slot", Some(arg_matches)) => {
                let slots = values_t_or_exit!(arg_matches, "slots", Slot);
                let blockstore =
                    open_blockstore(&ledger_path, AccessType::Primary, wal_recovery_mode);
                for slot in slots {
                    match blockstore.remove_dead_slot(slot) {
                        Ok(_) => println!("Slot {} not longer marked dead", slot),
                        Err(err) => {
                            eprintln!("Failed to remove dead flag for slot {}, {:?}", slot, err)
                        }
                    }
                }
            }
            ("parse_full_frozen", Some(arg_matches)) => {
                let starting_slot = value_t_or_exit!(arg_matches, "starting_slot", Slot);
                let ending_slot = value_t_or_exit!(arg_matches, "ending_slot", Slot);
                let blockstore =
                    open_blockstore(&ledger_path, AccessType::Secondary, wal_recovery_mode);
                let mut ancestors = BTreeSet::new();
                assert!(
                    blockstore.meta(ending_slot).unwrap().is_some(),
                    "Ending slot doesn't exist"
                );
                for a in AncestorIterator::new(ending_slot, &blockstore) {
                    ancestors.insert(a);
                    if a <= starting_slot {
                        break;
                    }
                }
                println!("ancestors: {:?}", ancestors.iter());

                let mut frozen = BTreeMap::new();
                let mut full = BTreeMap::new();
                let frozen_regex = Regex::new(r"bank frozen: (\d*)").unwrap();
                let full_regex = Regex::new(r"slot (\d*) is full").unwrap();

                let log_file = PathBuf::from(value_t_or_exit!(arg_matches, "log_path", String));
                let f = BufReader::new(File::open(log_file).unwrap());
                println!("Reading log file");
                for line in f.lines().flatten() {
                    let parse_results = {
                        if let Some(slot_string) = frozen_regex.captures_iter(&line).next() {
                            Some((slot_string, &mut frozen))
                        } else {
                            full_regex
                                .captures_iter(&line)
                                .next()
                                .map(|slot_string| (slot_string, &mut full))
                        }
                    };

                    if let Some((slot_string, map)) = parse_results {
                        let slot = slot_string
                            .get(1)
                            .expect("Only one match group")
                            .as_str()
                            .parse::<u64>()
                            .unwrap();
                        if ancestors.contains(&slot) && !map.contains_key(&slot) {
                            map.insert(slot, line);
                        }
                        if slot == ending_slot
                            && frozen.contains_key(&slot)
                            && full.contains_key(&slot)
                        {
                            break;
                        }
                    }
                }

                for ((slot1, frozen_log), (slot2, full_log)) in frozen.iter().zip(full.iter()) {
                    assert_eq!(slot1, slot2);
                    println!(
                        "Slot: {}\n, full: {}\n, frozen: {}",
                        slot1, full_log, frozen_log
                    );
                }
            }
            ("verify", Some(arg_matches)) => {
                let mut accounts_index_config = AccountsIndexConfig::default();
                if let Some(bins) = value_t!(arg_matches, "accounts_index_bins", usize).ok() {
                    accounts_index_config.bins = Some(bins);
                }

                let exit_signal = Arc::new(AtomicBool::new(false));

                let no_os_memory_stats_reporting =
                    arg_matches.is_present("no_os_memory_stats_reporting");
                let system_monitor_service = SystemMonitorService::new(
                    Arc::clone(&exit_signal),
                    !no_os_memory_stats_reporting,
                    false,
                );

                accounts_index_config.index_limit_mb = if let Some(limit) =
                    value_t!(arg_matches, "accounts_index_memory_limit_mb", usize).ok()
                {
                    IndexLimitMb::Limit(limit)
                } else if arg_matches.is_present("disable_accounts_disk_index") {
                    IndexLimitMb::InMemOnly
                } else {
                    IndexLimitMb::Unspecified
                };

                {
                    let mut accounts_index_paths: Vec<PathBuf> =
                        if arg_matches.is_present("accounts_index_path") {
                            values_t_or_exit!(arg_matches, "accounts_index_path", String)
                                .into_iter()
                                .map(PathBuf::from)
                                .collect()
                        } else {
                            vec![]
                        };
                    if accounts_index_paths.is_empty() {
                        accounts_index_paths = vec![ledger_path.join("accounts_index")];
                    }
                    accounts_index_config.drives = Some(accounts_index_paths);
                }

                let filler_accounts_config = FillerAccountsConfig {
                    count: value_t_or_exit!(arg_matches, "accounts_filler_count", usize),
                    size: value_t_or_exit!(arg_matches, "accounts_filler_size", usize),
                };

                let accounts_db_config = Some(AccountsDbConfig {
                    index: Some(accounts_index_config),
                    accounts_hash_cache_path: Some(ledger_path.clone()),
                    filler_accounts_config,
                    skip_rewrites: matches.is_present("accounts_db_skip_rewrites"),
                    ..AccountsDbConfig::default()
                });

                let process_options = ProcessOptions {
                    new_hard_forks: hardforks_of(arg_matches, "hard_forks"),
                    poh_verify: !arg_matches.is_present("skip_poh_verify"),
                    halt_at_slot: value_t!(arg_matches, "halt_at_slot", Slot).ok(),
                    accounts_db_caching_enabled: !arg_matches.is_present("no_accounts_db_caching"),
                    limit_load_slot_count_from_snapshot: value_t!(
                        arg_matches,
                        "limit_load_slot_count_from_snapshot",
                        usize
                    )
                    .ok(),
                    accounts_db_config,
                    verify_index: arg_matches.is_present("verify_accounts_index"),
                    allow_dead_slots: arg_matches.is_present("allow_dead_slots"),
                    accounts_db_test_hash_calculation: arg_matches
                        .is_present("accounts_db_test_hash_calculation"),
                    accounts_db_skip_shrink: arg_matches.is_present("accounts_db_skip_shrink"),
                    runtime_config: RuntimeConfig {
                        bpf_jit: !matches.is_present("no_bpf_jit"),
                        ..RuntimeConfig::default()
                    },
                    ..ProcessOptions::default()
                };
                let print_accounts_stats = arg_matches.is_present("print_accounts_stats");
                println!(
                    "genesis hash: {}",
                    open_genesis_config_by(&ledger_path, arg_matches).hash()
                );

                let blockstore =
                    open_blockstore(&ledger_path, AccessType::Secondary, wal_recovery_mode);
                let (bank_forks, ..) = load_bank_forks(
                    arg_matches,
                    &open_genesis_config_by(&ledger_path, arg_matches),
                    &blockstore,
                    process_options,
                    snapshot_archive_path,
                )
                .unwrap_or_else(|err| {
                    eprintln!("Ledger verification failed: {:?}", err);
                    exit(1);
                });
                if print_accounts_stats {
                    let working_bank = bank_forks.read().unwrap().working_bank();
                    working_bank.print_accounts_stats();
                }
                exit_signal.store(true, Ordering::Relaxed);
                system_monitor_service.join().unwrap();
                println!("Ok");
            }
            ("graph", Some(arg_matches)) => {
                let output_file = value_t_or_exit!(arg_matches, "graph_filename", String);

                let process_options = ProcessOptions {
                    new_hard_forks: hardforks_of(arg_matches, "hard_forks"),
                    halt_at_slot: value_t!(arg_matches, "halt_at_slot", Slot).ok(),
                    poh_verify: false,
                    ..ProcessOptions::default()
                };

                let blockstore =
                    open_blockstore(&ledger_path, AccessType::Secondary, wal_recovery_mode);
                match load_bank_forks(
                    arg_matches,
                    &open_genesis_config_by(&ledger_path, arg_matches),
                    &blockstore,
                    process_options,
                    snapshot_archive_path,
                ) {
                    Ok((bank_forks, ..)) => {
                        let dot = graph_forks(
                            &bank_forks.read().unwrap(),
                            arg_matches.is_present("include_all_votes"),
                        );

                        let extension = Path::new(&output_file).extension();
                        let result = if extension == Some(OsStr::new("pdf")) {
                            render_dot(dot, &output_file, "pdf")
                        } else if extension == Some(OsStr::new("png")) {
                            render_dot(dot, &output_file, "png")
                        } else {
                            File::create(&output_file)
                                .and_then(|mut file| file.write_all(&dot.into_bytes()))
                        };

                        match result {
                            Ok(_) => println!("Wrote {}", output_file),
                            Err(err) => eprintln!("Unable to write {}: {}", output_file, err),
                        }
                    }
                    Err(err) => {
                        eprintln!("Failed to load ledger: {:?}", err);
                        exit(1);
                    }
                }
            }
            ("create-snapshot", Some(arg_matches)) => {
                let output_directory = value_t!(arg_matches, "output_directory", PathBuf)
                    .unwrap_or_else(|_| match &snapshot_archive_path {
                        Some(snapshot_archive_path) => snapshot_archive_path.clone(),
                        None => ledger_path.clone(),
                    });
                let mut warp_slot = value_t!(arg_matches, "warp_slot", Slot).ok();
                let remove_stake_accounts = arg_matches.is_present("remove_stake_accounts");
                let new_hard_forks = hardforks_of(arg_matches, "hard_forks");

                let faucet_pubkey = pubkey_of(arg_matches, "faucet_pubkey");
                let faucet_lamports = value_t!(arg_matches, "faucet_lamports", u64).unwrap_or(0);

                let rent_burn_percentage = value_t!(arg_matches, "rent_burn_percentage", u8);
                let hashes_per_tick = arg_matches.value_of("hashes_per_tick");

                let bootstrap_stake_authorized_pubkey =
                    pubkey_of(arg_matches, "bootstrap_stake_authorized_pubkey");
                let bootstrap_validator_lamports =
                    value_t_or_exit!(arg_matches, "bootstrap_validator_lamports", u64);
                let bootstrap_validator_stake_lamports =
                    value_t_or_exit!(arg_matches, "bootstrap_validator_stake_lamports", u64);
                let minimum_stake_lamports = rent.minimum_balance(StakeState::size_of());
                if bootstrap_validator_stake_lamports < minimum_stake_lamports {
                    eprintln!(
                        "Error: insufficient --bootstrap-validator-stake-lamports. \
                           Minimum amount is {}",
                        minimum_stake_lamports
                    );
                    exit(1);
                }
                let bootstrap_validator_pubkeys = pubkeys_of(arg_matches, "bootstrap_validator");
                let accounts_to_remove =
                    pubkeys_of(arg_matches, "accounts_to_remove").unwrap_or_default();
                let vote_accounts_to_destake: HashSet<_> =
                    pubkeys_of(arg_matches, "vote_accounts_to_destake")
                        .unwrap_or_default()
                        .into_iter()
                        .collect();
                let snapshot_version = arg_matches.value_of("snapshot_version").map_or(
                    SnapshotVersion::default(),
                    |s| {
                        s.parse::<SnapshotVersion>().unwrap_or_else(|e| {
                            eprintln!("Error: {}", e);
                            exit(1)
                        })
                    },
                );

                let maximum_full_snapshot_archives_to_retain =
                    value_t_or_exit!(arg_matches, "maximum_full_snapshots_to_retain", usize);
                let maximum_incremental_snapshot_archives_to_retain = value_t_or_exit!(
                    arg_matches,
                    "maximum_incremental_snapshots_to_retain",
                    usize
                );
                let genesis_config = open_genesis_config_by(&ledger_path, arg_matches);
                let blockstore =
                    open_blockstore(&ledger_path, AccessType::Secondary, wal_recovery_mode);
                let is_incremental = arg_matches.is_present("incremental");

                let snapshot_slot = if Some("ROOT") == arg_matches.value_of("snapshot_slot") {
                    blockstore
                        .rooted_slot_iterator(0)
                        .expect("Failed to get rooted slot iterator")
                        .last()
                        .expect("Failed to get root")
                } else {
                    value_t_or_exit!(arg_matches, "snapshot_slot", Slot)
                };

                info!(
                    "Creating {}snapshot of slot {} in {}",
                    if is_incremental { "incremental " } else { "" },
                    snapshot_slot,
                    output_directory.display()
                );

                match load_bank_forks(
                    arg_matches,
                    &genesis_config,
                    &blockstore,
                    ProcessOptions {
                        new_hard_forks,
                        halt_at_slot: Some(snapshot_slot),
                        poh_verify: false,
                        ..ProcessOptions::default()
                    },
                    snapshot_archive_path,
                ) {
                    Ok((bank_forks, starting_snapshot_hashes)) => {
                        let mut bank = bank_forks
                            .read()
                            .unwrap()
                            .get(snapshot_slot)
                            .unwrap_or_else(|| {
                                eprintln!("Error: Slot {} is not available", snapshot_slot);
                                exit(1);
                            });

                        let child_bank_required = rent_burn_percentage.is_ok()
                            || hashes_per_tick.is_some()
                            || remove_stake_accounts
                            || !accounts_to_remove.is_empty()
                            || !vote_accounts_to_destake.is_empty()
                            || faucet_pubkey.is_some()
                            || bootstrap_validator_pubkeys.is_some();

                        if child_bank_required {
                            let mut child_bank =
                                Bank::new_from_parent(&bank, bank.collector_id(), bank.slot() + 1);

                            if let Ok(rent_burn_percentage) = rent_burn_percentage {
                                child_bank.set_rent_burn_percentage(rent_burn_percentage);
                            }

                            if let Some(hashes_per_tick) = hashes_per_tick {
                                child_bank.set_hashes_per_tick(match hashes_per_tick {
                                    // Note: Unlike `solana-genesis`, "auto" is not supported here.
                                    "sleep" => None,
                                    _ => {
                                        Some(value_t_or_exit!(arg_matches, "hashes_per_tick", u64))
                                    }
                                });
                            }
                            bank = Arc::new(child_bank);
                        }

                        if let Some(faucet_pubkey) = faucet_pubkey {
                            bank.store_account(
                                &faucet_pubkey,
                                &AccountSharedData::new(faucet_lamports, 0, &system_program::id()),
                            );
                        }

                        if remove_stake_accounts {
                            for (address, mut account) in bank
                                .get_program_accounts(&stake::program::id(), &ScanConfig::default())
                                .unwrap()
                                .into_iter()
                            {
                                account.set_lamports(0);
                                bank.store_account(&address, &account);
                            }
                        }

                        for address in accounts_to_remove {
                            let mut account = bank.get_account(&address).unwrap_or_else(|| {
                                eprintln!(
                                    "Error: Account does not exist, unable to remove it: {}",
                                    address
                                );
                                exit(1);
                            });

                            account.set_lamports(0);
                            bank.store_account(&address, &account);
                        }

                        if !vote_accounts_to_destake.is_empty() {
                            for (address, mut account) in bank
                                .get_program_accounts(&stake::program::id(), &ScanConfig::default())
                                .unwrap()
                                .into_iter()
                            {
                                if let Ok(StakeState::Stake(meta, stake)) = account.state() {
                                    if vote_accounts_to_destake
                                        .contains(&stake.delegation.voter_pubkey)
                                    {
                                        if verbose_level > 0 {
                                            warn!(
                                                "Undelegating stake account {} from {}",
                                                address, stake.delegation.voter_pubkey,
                                            );
                                        }
                                        account.set_state(&StakeState::Initialized(meta)).unwrap();
                                        bank.store_account(&address, &account);
                                    }
                                }
                            }
                        }

                        if let Some(bootstrap_validator_pubkeys) = bootstrap_validator_pubkeys {
                            assert_eq!(bootstrap_validator_pubkeys.len() % 3, 0);

                            // Ensure there are no duplicated pubkeys in the --bootstrap-validator list
                            {
                                let mut v = bootstrap_validator_pubkeys.clone();
                                v.sort();
                                v.dedup();
                                if v.len() != bootstrap_validator_pubkeys.len() {
                                    eprintln!(
                                        "Error: --bootstrap-validator pubkeys cannot be duplicated"
                                    );
                                    exit(1);
                                }
                            }

                            // Delete existing vote accounts
                            for (address, mut account) in bank
                                .get_program_accounts(
                                    &solana_vote_program::id(),
                                    &ScanConfig::default(),
                                )
                                .unwrap()
                                .into_iter()
                            {
                                account.set_lamports(0);
                                bank.store_account(&address, &account);
                            }

                            // Add a new identity/vote/stake account for each of the provided bootstrap
                            // validators
                            let mut bootstrap_validator_pubkeys_iter =
                                bootstrap_validator_pubkeys.iter();
                            loop {
                                let identity_pubkey = match bootstrap_validator_pubkeys_iter.next()
                                {
                                    None => break,
                                    Some(identity_pubkey) => identity_pubkey,
                                };
                                let vote_pubkey = bootstrap_validator_pubkeys_iter.next().unwrap();
                                let stake_pubkey = bootstrap_validator_pubkeys_iter.next().unwrap();

                                bank.store_account(
                                    identity_pubkey,
                                    &AccountSharedData::new(
                                        bootstrap_validator_lamports,
                                        0,
                                        &system_program::id(),
                                    ),
                                );

                                let vote_account = vote_state::create_account_with_authorized(
                                    identity_pubkey,
                                    identity_pubkey,
                                    identity_pubkey,
                                    100,
                                    VoteState::get_rent_exempt_reserve(&rent).max(1),
                                );

                                bank.store_account(
                                    stake_pubkey,
                                    &stake_state::create_account(
                                        bootstrap_stake_authorized_pubkey
                                            .as_ref()
                                            .unwrap_or(identity_pubkey),
                                        vote_pubkey,
                                        &vote_account,
                                        &rent,
                                        bootstrap_validator_stake_lamports,
                                    ),
                                );
                                bank.store_account(vote_pubkey, &vote_account);
                            }

                            // Warp ahead at least two epochs to ensure that the leader schedule will be
                            // updated to reflect the new bootstrap validator(s)
                            let minimum_warp_slot =
                                genesis_config.epoch_schedule.get_first_slot_in_epoch(
                                    genesis_config.epoch_schedule.get_epoch(snapshot_slot) + 2,
                                );

                            if let Some(warp_slot) = warp_slot {
                                if warp_slot < minimum_warp_slot {
                                    eprintln!(
                                        "Error: --warp-slot too close.  Must be >= {}",
                                        minimum_warp_slot
                                    );
                                    exit(1);
                                }
                            } else {
                                warn!("Warping to slot {}", minimum_warp_slot);
                                warp_slot = Some(minimum_warp_slot);
                            }
                        }

                        if child_bank_required {
                            while !bank.is_complete() {
                                bank.register_tick(&Hash::new_unique());
                            }
                        }

                        bank.set_capitalization();

                        let bank = if let Some(warp_slot) = warp_slot {
                            Arc::new(Bank::warp_from_parent(
                                &bank,
                                bank.collector_id(),
                                warp_slot,
                            ))
                        } else {
                            bank
                        };

                        println!(
                            "Creating a version {} {}snapshot of slot {}",
                            snapshot_version,
                            if is_incremental { "incremental " } else { "" },
                            bank.slot(),
                        );

                        if is_incremental {
                            if starting_snapshot_hashes.is_none() {
                                eprintln!("Unable to create incremental snapshot without a base full snapshot");
                                exit(1);
                            }
                            let full_snapshot_slot = starting_snapshot_hashes.unwrap().full.hash.0;
                            if bank.slot() <= full_snapshot_slot {
                                eprintln!(
                                    "Unable to create incremental snapshot: Slot must be greater than full snapshot slot. slot: {}, full snapshot slot: {}",
                                    bank.slot(),
                                    full_snapshot_slot,
                                );
                                exit(1);
                            }

                            let incremental_snapshot_archive_info =
                                snapshot_utils::bank_to_incremental_snapshot_archive(
                                    ledger_path,
                                    &bank,
                                    full_snapshot_slot,
                                    Some(snapshot_version),
                                    output_directory,
                                    ArchiveFormat::TarZstd,
                                    maximum_full_snapshot_archives_to_retain,
                                    maximum_incremental_snapshot_archives_to_retain,
                                )
                                .unwrap_or_else(|err| {
                                    eprintln!("Unable to create incremental snapshot: {}", err);
                                    exit(1);
                                });

                            println!(
                                "Successfully created incremental snapshot for slot {}, hash {}, base slot: {}: {}",
                                bank.slot(),
                                bank.hash(),
                                full_snapshot_slot,
                                incremental_snapshot_archive_info.path().display(),
                            );
                        } else {
                            let full_snapshot_archive_info =
                                snapshot_utils::bank_to_full_snapshot_archive(
                                    ledger_path,
                                    &bank,
                                    Some(snapshot_version),
                                    output_directory,
                                    ArchiveFormat::TarZstd,
                                    maximum_full_snapshot_archives_to_retain,
                                    maximum_incremental_snapshot_archives_to_retain,
                                )
                                .unwrap_or_else(|err| {
                                    eprintln!("Unable to create snapshot: {}", err);
                                    exit(1);
                                });

                            println!(
                                "Successfully created snapshot for slot {}, hash {}: {}",
                                bank.slot(),
                                bank.hash(),
                                full_snapshot_archive_info.path().display(),
                            );
                        }

                        println!(
                            "Shred version: {}",
                            compute_shred_version(
                                &genesis_config.hash(),
                                Some(&bank.hard_forks().read().unwrap())
                            )
                        );
                    }
                    Err(err) => {
                        eprintln!("Failed to load ledger: {:?}", err);
                        exit(1);
                    }
                }
            }
            ("accounts", Some(arg_matches)) => {
                let halt_at_slot = value_t!(arg_matches, "halt_at_slot", Slot).ok();
                let process_options = ProcessOptions {
                    new_hard_forks: hardforks_of(arg_matches, "hard_forks"),
                    halt_at_slot,
                    poh_verify: false,
                    ..ProcessOptions::default()
                };
                let genesis_config = open_genesis_config_by(&ledger_path, arg_matches);
                let include_sysvars = arg_matches.is_present("include_sysvars");
                let blockstore =
                    open_blockstore(&ledger_path, AccessType::Secondary, wal_recovery_mode);
                let (bank_forks, ..) = load_bank_forks(
                    arg_matches,
                    &genesis_config,
                    &blockstore,
                    process_options,
                    snapshot_archive_path,
                )
                .unwrap_or_else(|err| {
                    eprintln!("Failed to load ledger: {:?}", err);
                    exit(1);
                });

                let bank = bank_forks.read().unwrap().working_bank();
                let mut measure = Measure::start("getting accounts");
                let accounts: BTreeMap<_, _> = bank
                    .get_all_accounts_with_modified_slots()
                    .unwrap()
                    .into_iter()
                    .filter(|(pubkey, _account, _slot)| {
                        include_sysvars || !solana_sdk::sysvar::is_sysvar_id(pubkey)
                    })
                    .map(|(pubkey, account, slot)| (pubkey, (account, slot)))
                    .collect();
                measure.stop();
                info!("{}", measure);

                let mut measure = Measure::start("calculating total accounts stats");
                let total_accounts_stats = bank.calculate_total_accounts_stats(
                    accounts
                        .iter()
                        .map(|(pubkey, (account, _slot))| (pubkey, account)),
                );
                measure.stop();
                info!("{}", measure);

                let print_account_contents = !arg_matches.is_present("no_account_contents");
                if print_account_contents {
                    let print_account_data = !arg_matches.is_present("no_account_data");
                    let mut measure = Measure::start("printing account contents");
                    for (pubkey, (account, slot)) in accounts.into_iter() {
                        output_account(&pubkey, &account, Some(slot), print_account_data);
                    }
                    measure.stop();
                    info!("{}", measure);
                }

                println!("{:#?}", total_accounts_stats);
            }
            ("capitalization", Some(arg_matches)) => {
                let halt_at_slot = value_t!(arg_matches, "halt_at_slot", Slot).ok();
                let process_options = ProcessOptions {
                    new_hard_forks: hardforks_of(arg_matches, "hard_forks"),
                    halt_at_slot,
                    poh_verify: false,
                    ..ProcessOptions::default()
                };
                let genesis_config = open_genesis_config_by(&ledger_path, arg_matches);
                let blockstore =
                    open_blockstore(&ledger_path, AccessType::Secondary, wal_recovery_mode);
                match load_bank_forks(
                    arg_matches,
                    &genesis_config,
                    &blockstore,
                    process_options,
                    snapshot_archive_path,
                ) {
                    Ok((bank_forks, ..)) => {
                        let bank_forks = bank_forks.read().unwrap();
                        let slot = bank_forks.working_bank().slot();
                        let bank = bank_forks.get(slot).unwrap_or_else(|| {
                            eprintln!("Error: Slot {} is not available", slot);
                            exit(1);
                        });

                        if arg_matches.is_present("recalculate_capitalization") {
                            println!("Recalculating capitalization");
                            let old_capitalization = bank.set_capitalization();
                            if old_capitalization == bank.capitalization() {
                                eprintln!(
                                    "Capitalization was identical: {}",
                                    Sol(old_capitalization)
                                );
                            }
                        }

                        if arg_matches.is_present("warp_epoch") {
                            let base_bank = bank;

                            let raw_warp_epoch =
                                value_t!(arg_matches, "warp_epoch", String).unwrap();
                            let warp_epoch = if raw_warp_epoch.starts_with('+') {
                                base_bank.epoch()
                                    + value_t!(arg_matches, "warp_epoch", Epoch).unwrap()
                            } else {
                                value_t!(arg_matches, "warp_epoch", Epoch).unwrap()
                            };
                            if warp_epoch < base_bank.epoch() {
                                eprintln!(
                                    "Error: can't warp epoch backwards: {} => {}",
                                    base_bank.epoch(),
                                    warp_epoch
                                );
                                exit(1);
                            }

                            if let Ok(raw_inflation) = value_t!(arg_matches, "inflation", String) {
                                let inflation = match raw_inflation.as_str() {
                                    "pico" => Inflation::pico(),
                                    "full" => Inflation::full(),
                                    "none" => Inflation::new_disabled(),
                                    _ => unreachable!(),
                                };
                                println!(
                                    "Forcing to: {:?} (was: {:?})",
                                    inflation,
                                    base_bank.inflation()
                                );
                                base_bank.set_inflation(inflation);
                            }

                            let next_epoch = base_bank
                                .epoch_schedule()
                                .get_first_slot_in_epoch(warp_epoch);
                            // disable eager rent collection because this creates many unrelated
                            // rent collection account updates
                            base_bank
                                .lazy_rent_collection
                                .store(true, std::sync::atomic::Ordering::Relaxed);

                            let feature_account_balance = std::cmp::max(
                                genesis_config.rent.minimum_balance(Feature::size_of()),
                                1,
                            );
                            if arg_matches.is_present("enable_credits_auto_rewind") {
                                base_bank.unfreeze_for_ledger_tool();
                                let mut force_enabled_count = 0;
                                if base_bank
                                    .get_account(&feature_set::credits_auto_rewind::id())
                                    .is_none()
                                {
                                    base_bank.store_account(
                                        &feature_set::credits_auto_rewind::id(),
                                        &feature::create_account(
                                            &Feature { activated_at: None },
                                            feature_account_balance,
                                        ),
                                    );
                                    force_enabled_count += 1;
                                }
                                if force_enabled_count == 0 {
                                    warn!(
                                        "Already credits_auto_rewind is activated (or scheduled)"
                                    );
                                }
                                let mut store_failed_count = 0;
                                if force_enabled_count >= 1 {
                                    if base_bank
                                        .get_account(&feature_set::deprecate_rewards_sysvar::id())
                                        .is_some()
                                    {
                                        // steal some lamports from the pretty old feature not to affect
                                        // capitalizaion, which doesn't affect inflation behavior!
                                        base_bank.store_account(
                                            &feature_set::deprecate_rewards_sysvar::id(),
                                            &AccountSharedData::default(),
                                        );
                                        force_enabled_count -= 1;
                                    } else {
                                        store_failed_count += 1;
                                    }
                                }
                                assert_eq!(force_enabled_count, store_failed_count);
                                if store_failed_count >= 1 {
                                    // we have no choice; maybe locally created blank cluster with
                                    // not-Development cluster type.
                                    let old_cap = base_bank.set_capitalization();
                                    let new_cap = base_bank.capitalization();
                                    warn!(
                                        "Skewing capitalization a bit to enable credits_auto_rewind as \
                                         requested: increasing {} from {} to {}",
                                        feature_account_balance, old_cap, new_cap,
                                    );
                                    assert_eq!(
                                        old_cap + feature_account_balance * store_failed_count,
                                        new_cap
                                    );
                                }
                            }

                            #[derive(Default, Debug)]
                            struct PointDetail {
                                epoch: Epoch,
                                points: u128,
                                stake: u128,
                                credits: u128,
                            }

                            #[derive(Default, Debug)]
                            struct CalculationDetail {
                                epochs: usize,
                                voter: Pubkey,
                                voter_owner: Pubkey,
                                current_effective_stake: u64,
                                total_stake: u64,
                                rent_exempt_reserve: u64,
                                points: Vec<PointDetail>,
                                base_rewards: u64,
                                commission: u8,
                                vote_rewards: u64,
                                stake_rewards: u64,
                                activation_epoch: Epoch,
                                deactivation_epoch: Option<Epoch>,
                                point_value: Option<PointValue>,
                                old_credits_observed: Option<u64>,
                                new_credits_observed: Option<u64>,
                                skipped_reasons: String,
                            }
                            use solana_stake_program::stake_state::InflationPointCalculationEvent;
                            let stake_calculation_details: DashMap<Pubkey, CalculationDetail> =
                                DashMap::new();
                            let last_point_value = Arc::new(RwLock::new(None));
                            let tracer = |event: &RewardCalculationEvent| {
                                // Currently RewardCalculationEvent enum has only Staking variant
                                // because only staking tracing is supported!
                                #[allow(irrefutable_let_patterns)]
                                if let RewardCalculationEvent::Staking(pubkey, event) = event {
                                    let mut detail =
                                        stake_calculation_details.entry(**pubkey).or_default();
                                    match event {
                                    InflationPointCalculationEvent::CalculatedPoints(
                                        epoch,
                                        stake,
                                        credits,
                                        points,
                                    ) => {
                                        if *points > 0 {
                                            detail.epochs += 1;
                                            detail.points.push(PointDetail {epoch: *epoch, points: *points, stake: *stake, credits: *credits});
                                        }
                                    }
                                    InflationPointCalculationEvent::SplitRewards(
                                        all,
                                        voter,
                                        staker,
                                        point_value,
                                    ) => {
                                        detail.base_rewards = *all;
                                        detail.vote_rewards = *voter;
                                        detail.stake_rewards = *staker;
                                        detail.point_value = Some(point_value.clone());
                                        // we have duplicate copies of `PointValue`s for possible
                                        // miscalculation; do some minimum sanity check
                                        let mut last_point_value = last_point_value.write().unwrap();
                                        if let Some(last_point_value) = last_point_value.as_ref() {
                                            assert_eq!(last_point_value, point_value);
                                        } else {
                                            *last_point_value = Some(point_value.clone());
                                        }
                                    }
                                    InflationPointCalculationEvent::EffectiveStakeAtRewardedEpoch(stake) => {
                                        detail.current_effective_stake = *stake;
                                    }
                                    InflationPointCalculationEvent::Commission(commission) => {
                                        detail.commission = *commission;
                                    }
                                    InflationPointCalculationEvent::RentExemptReserve(reserve) => {
                                        detail.rent_exempt_reserve = *reserve;
                                    }
                                    InflationPointCalculationEvent::CreditsObserved(
                                        old_credits_observed,
                                        new_credits_observed,
                                    ) => {
                                        detail.old_credits_observed = Some(*old_credits_observed);
                                        detail.new_credits_observed = *new_credits_observed;
                                    }
                                    InflationPointCalculationEvent::Delegation(
                                        delegation,
                                        owner,
                                    ) => {
                                        detail.voter = delegation.voter_pubkey;
                                        detail.voter_owner = *owner;
                                        detail.total_stake = delegation.stake;
                                        detail.activation_epoch = delegation.activation_epoch;
                                        if delegation.deactivation_epoch < Epoch::max_value() {
                                            detail.deactivation_epoch =
                                                Some(delegation.deactivation_epoch);
                                        }
                                    }
                                    InflationPointCalculationEvent::Skipped(skipped_reason) => {
                                        if detail.skipped_reasons.is_empty() {
                                            detail.skipped_reasons = format!("{:?}", skipped_reason);
                                        } else {
                                            detail.skipped_reasons += &format!("/{:?}", skipped_reason);
                                        }
                                    }
                                }
                                }
                            };
                            let warped_bank = Bank::new_from_parent_with_tracer(
                                &base_bank,
                                base_bank.collector_id(),
                                next_epoch,
                                tracer,
                            );
                            warped_bank.freeze();
                            let mut csv_writer = if arg_matches.is_present("csv_filename") {
                                let csv_filename =
                                    value_t_or_exit!(arg_matches, "csv_filename", String);
                                let file = File::create(&csv_filename).unwrap();
                                Some(csv::WriterBuilder::new().from_writer(file))
                            } else {
                                None
                            };

                            println!("Slot: {} => {}", base_bank.slot(), warped_bank.slot());
                            println!("Epoch: {} => {}", base_bank.epoch(), warped_bank.epoch());
                            assert_capitalization(&base_bank);
                            assert_capitalization(&warped_bank);
                            let interest_per_epoch = ((warped_bank.capitalization() as f64)
                                / (base_bank.capitalization() as f64)
                                * 100_f64)
                                - 100_f64;
                            let interest_per_year = interest_per_epoch
                                / warped_bank.epoch_duration_in_years(base_bank.epoch());
                            println!(
                                "Capitalization: {} => {} (+{} {}%; annualized {}%)",
                                Sol(base_bank.capitalization()),
                                Sol(warped_bank.capitalization()),
                                Sol(warped_bank.capitalization() - base_bank.capitalization()),
                                interest_per_epoch,
                                interest_per_year,
                            );

                            let mut overall_delta = 0;

                            let modified_accounts =
                                warped_bank.get_all_accounts_modified_since_parent();
                            let mut rewarded_accounts = modified_accounts
                                .iter()
                                .map(|(pubkey, account)| {
                                    (
                                        pubkey,
                                        account,
                                        base_bank
                                            .get_account(pubkey)
                                            .map(|a| a.lamports())
                                            .unwrap_or_default(),
                                    )
                                })
                                .collect::<Vec<_>>();
                            rewarded_accounts.sort_unstable_by_key(
                                |(pubkey, account, base_lamports)| {
                                    (
                                        *account.owner(),
                                        *base_lamports,
                                        account.lamports() - base_lamports,
                                        *pubkey,
                                    )
                                },
                            );

                            let mut unchanged_accounts = stake_calculation_details
                                .iter()
                                .map(|entry| *entry.key())
                                .collect::<HashSet<_>>()
                                .difference(
                                    &rewarded_accounts
                                        .iter()
                                        .map(|(pubkey, ..)| **pubkey)
                                        .collect(),
                                )
                                .map(|pubkey| (*pubkey, warped_bank.get_account(pubkey).unwrap()))
                                .collect::<Vec<_>>();
                            unchanged_accounts.sort_unstable_by_key(|(pubkey, account)| {
                                (*account.owner(), account.lamports(), *pubkey)
                            });
                            let unchanged_accounts = unchanged_accounts.into_iter();

                            let rewarded_accounts = rewarded_accounts
                                .into_iter()
                                .map(|(pubkey, account, ..)| (*pubkey, account.clone()));

                            let all_accounts = unchanged_accounts.chain(rewarded_accounts);
                            for (pubkey, warped_account) in all_accounts {
                                // Don't output sysvars; it's always updated but not related to
                                // inflation.
                                if solana_sdk::sysvar::is_sysvar_id(&pubkey) {
                                    continue;
                                }

                                if let Some(base_account) = base_bank.get_account(&pubkey) {
                                    let delta = warped_account.lamports() - base_account.lamports();
                                    let detail_ref = stake_calculation_details.get(&pubkey);
                                    let detail: Option<&CalculationDetail> =
                                        detail_ref.as_ref().map(|detail_ref| detail_ref.value());
                                    println!(
                                        "{:<45}({}): {} => {} (+{} {:>4.9}%) {:?}",
                                        format!("{}", pubkey), // format! is needed to pad/justify correctly.
                                        base_account.owner(),
                                        Sol(base_account.lamports()),
                                        Sol(warped_account.lamports()),
                                        Sol(delta),
                                        ((warped_account.lamports() as f64)
                                            / (base_account.lamports() as f64)
                                            * 100_f64)
                                            - 100_f64,
                                        detail,
                                    );
                                    if let Some(ref mut csv_writer) = csv_writer {
                                        #[derive(Serialize)]
                                        struct InflationRecord {
                                            cluster_type: String,
                                            rewarded_epoch: Epoch,
                                            account: String,
                                            owner: String,
                                            old_balance: u64,
                                            new_balance: u64,
                                            data_size: usize,
                                            delegation: String,
                                            delegation_owner: String,
                                            effective_stake: String,
                                            delegated_stake: String,
                                            rent_exempt_reserve: String,
                                            activation_epoch: String,
                                            deactivation_epoch: String,
                                            earned_epochs: String,
                                            epoch: String,
                                            epoch_credits: String,
                                            epoch_points: String,
                                            epoch_stake: String,
                                            old_credits_observed: String,
                                            new_credits_observed: String,
                                            base_rewards: String,
                                            stake_rewards: String,
                                            vote_rewards: String,
                                            commission: String,
                                            cluster_rewards: String,
                                            cluster_points: String,
                                            old_capitalization: u64,
                                            new_capitalization: u64,
                                        }
                                        fn format_or_na<T: std::fmt::Display>(
                                            data: Option<T>,
                                        ) -> String {
                                            data.map(|data| format!("{}", data))
                                                .unwrap_or_else(|| "N/A".to_owned())
                                        }
                                        let mut point_details = detail
                                            .map(|d| d.points.iter().map(Some).collect::<Vec<_>>())
                                            .unwrap_or_default();

                                        // ensure to print even if there is no calculation/point detail
                                        if point_details.is_empty() {
                                            point_details.push(None);
                                        }

                                        for point_detail in point_details {
                                            let record = InflationRecord {
                                                cluster_type: format!(
                                                    "{:?}",
                                                    base_bank.cluster_type()
                                                ),
                                                rewarded_epoch: base_bank.epoch(),
                                                account: format!("{}", pubkey),
                                                owner: format!("{}", base_account.owner()),
                                                old_balance: base_account.lamports(),
                                                new_balance: warped_account.lamports(),
                                                data_size: base_account.data().len(),
                                                delegation: format_or_na(detail.map(|d| d.voter)),
                                                delegation_owner: format_or_na(
                                                    detail.map(|d| d.voter_owner),
                                                ),
                                                effective_stake: format_or_na(
                                                    detail.map(|d| d.current_effective_stake),
                                                ),
                                                delegated_stake: format_or_na(
                                                    detail.map(|d| d.total_stake),
                                                ),
                                                rent_exempt_reserve: format_or_na(
                                                    detail.map(|d| d.rent_exempt_reserve),
                                                ),
                                                activation_epoch: format_or_na(detail.map(|d| {
                                                    if d.activation_epoch < Epoch::max_value() {
                                                        d.activation_epoch
                                                    } else {
                                                        // bootstraped
                                                        0
                                                    }
                                                })),
                                                deactivation_epoch: format_or_na(
                                                    detail.and_then(|d| d.deactivation_epoch),
                                                ),
                                                earned_epochs: format_or_na(
                                                    detail.map(|d| d.epochs),
                                                ),
                                                epoch: format_or_na(point_detail.map(|d| d.epoch)),
                                                epoch_credits: format_or_na(
                                                    point_detail.map(|d| d.credits),
                                                ),
                                                epoch_points: format_or_na(
                                                    point_detail.map(|d| d.points),
                                                ),
                                                epoch_stake: format_or_na(
                                                    point_detail.map(|d| d.stake),
                                                ),
                                                old_credits_observed: format_or_na(
                                                    detail.and_then(|d| d.old_credits_observed),
                                                ),
                                                new_credits_observed: format_or_na(
                                                    detail.and_then(|d| d.new_credits_observed),
                                                ),
                                                base_rewards: format_or_na(
                                                    detail.map(|d| d.base_rewards),
                                                ),
                                                stake_rewards: format_or_na(
                                                    detail.map(|d| d.stake_rewards),
                                                ),
                                                vote_rewards: format_or_na(
                                                    detail.map(|d| d.vote_rewards),
                                                ),
                                                commission: format_or_na(
                                                    detail.map(|d| d.commission),
                                                ),
                                                cluster_rewards: format_or_na(
                                                    last_point_value
                                                        .read()
                                                        .unwrap()
                                                        .clone()
                                                        .map(|pv| pv.rewards),
                                                ),
                                                cluster_points: format_or_na(
                                                    last_point_value
                                                        .read()
                                                        .unwrap()
                                                        .clone()
                                                        .map(|pv| pv.points),
                                                ),
                                                old_capitalization: base_bank.capitalization(),
                                                new_capitalization: warped_bank.capitalization(),
                                            };
                                            csv_writer.serialize(&record).unwrap();
                                        }
                                    }
                                    overall_delta += delta;
                                } else {
                                    error!("new account!?: {}", pubkey);
                                }
                            }
                            if overall_delta > 0 {
                                println!("Sum of lamports changes: {}", Sol(overall_delta));
                            }
                        } else {
                            if arg_matches.is_present("recalculate_capitalization") {
                                eprintln!(
                                    "Capitalization isn't verified because it's recalculated"
                                );
                            }
                            if arg_matches.is_present("inflation") {
                                eprintln!(
                                    "Forcing inflation isn't meaningful because bank isn't warping"
                                );
                            }

                            assert_capitalization(&bank);
                            println!("Inflation: {:?}", bank.inflation());
                            println!("RentCollector: {:?}", bank.rent_collector());
                            println!("Capitalization: {}", Sol(bank.capitalization()));
                        }
                    }
                    Err(err) => {
                        eprintln!("Failed to load ledger: {:?}", err);
                        exit(1);
                    }
                }
            }
            ("purge", Some(arg_matches)) => {
                let start_slot = value_t_or_exit!(arg_matches, "start_slot", Slot);
                let end_slot = value_t!(arg_matches, "end_slot", Slot).ok();
                let no_compaction = arg_matches.is_present("no_compaction");
                let dead_slots_only = arg_matches.is_present("dead_slots_only");
                let batch_size = value_t_or_exit!(arg_matches, "batch_size", usize);
                let access_type = if !no_compaction {
                    AccessType::Primary
                } else {
                    AccessType::PrimaryForMaintenance
                };
                let blockstore = open_blockstore(&ledger_path, access_type, wal_recovery_mode);

                let end_slot = match end_slot {
                    Some(end_slot) => end_slot,
                    None => match blockstore.slot_meta_iterator(start_slot) {
                        Ok(metas) => {
                            let slots: Vec<_> = metas.map(|(slot, _)| slot).collect();
                            if slots.is_empty() {
                                eprintln!("Purge range is empty");
                                exit(1);
                            }
                            *slots.last().unwrap()
                        }
                        Err(err) => {
                            eprintln!("Unable to read the Ledger: {:?}", err);
                            exit(1);
                        }
                    },
                };

                if end_slot < start_slot {
                    eprintln!(
                        "end slot {} is less than start slot {}",
                        end_slot, start_slot
                    );
                    exit(1);
                }
                info!(
                "Purging data from slots {} to {} ({} slots) (skip compaction: {}) (dead slot only: {})",
                start_slot,
                end_slot,
                end_slot - start_slot,
                no_compaction,
                dead_slots_only,
            );
                let purge_from_blockstore = |start_slot, end_slot| {
                    blockstore.purge_from_next_slots(start_slot, end_slot);
                    if no_compaction {
                        blockstore.purge_slots(start_slot, end_slot, PurgeType::Exact);
                    } else {
                        blockstore.purge_and_compact_slots(start_slot, end_slot);
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
            ("list-roots", Some(arg_matches)) => {
                let blockstore =
                    open_blockstore(&ledger_path, AccessType::Secondary, wal_recovery_mode);
                let max_height = if let Some(height) = arg_matches.value_of("max_height") {
                    usize::from_str(height).expect("Maximum height must be a number")
                } else {
                    usize::MAX
                };
                let start_root = if let Some(height) = arg_matches.value_of("start_root") {
                    Slot::from_str(height).expect("Starting root must be a number")
                } else {
                    0
                };
                let num_roots = if let Some(roots) = arg_matches.value_of("num_roots") {
                    usize::from_str(roots).expect("Number of roots must be a number")
                } else {
                    usize::from_str(DEFAULT_ROOT_COUNT).unwrap()
                };

                let iter = blockstore
                    .rooted_slot_iterator(start_root)
                    .expect("Failed to get rooted slot");

                let mut slot_hash = Vec::new();
                for (i, slot) in iter.into_iter().enumerate() {
                    if i > num_roots {
                        break;
                    }
                    if slot <= max_height as u64 {
                        let blockhash = blockstore
                            .get_slot_entries(slot, 0)
                            .unwrap()
                            .last()
                            .unwrap()
                            .hash;
                        slot_hash.push((slot, blockhash));
                    } else {
                        break;
                    }
                }

                let mut output_file: Box<dyn Write> =
                    if let Some(path) = arg_matches.value_of("slot_list") {
                        match File::create(path) {
                            Ok(file) => Box::new(file),
                            _ => Box::new(stdout()),
                        }
                    } else {
                        Box::new(stdout())
                    };

                slot_hash
                    .into_iter()
                    .rev()
                    .enumerate()
                    .for_each(|(i, (slot, hash))| {
                        if i < num_roots {
                            output_file
                                .write_all(format!("{:?}: {:?}\n", slot, hash).as_bytes())
                                .expect("failed to write");
                        }
                    });
            }
            ("repair-roots", Some(arg_matches)) => {
                let blockstore =
                    open_blockstore(&ledger_path, AccessType::Primary, wal_recovery_mode);
                let start_root = if let Some(root) = arg_matches.value_of("start_root") {
                    Slot::from_str(root).expect("Before root must be a number")
                } else {
                    blockstore.max_root()
                };
                let max_slots = value_t_or_exit!(arg_matches, "max_slots", u64);
                let end_root = if let Some(root) = arg_matches.value_of("end_root") {
                    Slot::from_str(root).expect("Until root must be a number")
                } else {
                    start_root.saturating_sub(max_slots)
                };
                assert!(start_root > end_root);
                assert!(blockstore.is_root(start_root));
                let num_slots = start_root - end_root - 1; // Adjust by one since start_root need not be checked
                if arg_matches.is_present("end_root") && num_slots > max_slots {
                    eprintln!(
                        "Requested range {} too large, max {}. \
                    Either adjust `--until` value, or pass a larger `--repair-limit` \
                    to override the limit",
                        num_slots, max_slots,
                    );
                    exit(1);
                }
                let ancestor_iterator = AncestorIterator::new(start_root, &blockstore)
                    .take_while(|&slot| slot >= end_root);
                let roots_to_fix: Vec<_> = ancestor_iterator
                    .filter(|slot| !blockstore.is_root(*slot))
                    .collect();
                if !roots_to_fix.is_empty() {
                    eprintln!("{} slots to be rooted", roots_to_fix.len());
                    for chunk in roots_to_fix.chunks(100) {
                        eprintln!("{:?}", chunk);
                        blockstore
                            .set_roots(roots_to_fix.iter())
                            .unwrap_or_else(|err| {
                                eprintln!("Unable to set roots {:?}: {}", roots_to_fix, err);
                                exit(1);
                            });
                    }
                } else {
                    println!(
                        "No missing roots found in range {} to {}",
                        end_root, start_root
                    );
                }
            }
            ("bounds", Some(arg_matches)) => {
                let blockstore =
                    open_blockstore(&ledger_path, AccessType::Secondary, wal_recovery_mode);
                match blockstore.slot_meta_iterator(0) {
                    Ok(metas) => {
                        let all = arg_matches.is_present("all");

                        let slots: Vec<_> = metas.map(|(slot, _)| slot).collect();
                        if slots.is_empty() {
                            println!("Ledger is empty");
                        } else {
                            let first = slots.first().unwrap();
                            let last = slots.last().unwrap_or(first);
                            if first != last {
                                println!(
                                    "Ledger has data for {} slots {:?} to {:?}",
                                    slots.len(),
                                    first,
                                    last
                                );
                                if all {
                                    println!("Non-empty slots: {:?}", slots);
                                }
                            } else {
                                println!("Ledger has data for slot {:?}", first);
                            }
                        }
                        if let Ok(rooted) = blockstore.rooted_slot_iterator(0) {
                            let mut first_rooted = 0;
                            let mut last_rooted = 0;
                            let mut total_rooted = 0;
                            for (i, slot) in rooted.into_iter().enumerate() {
                                if i == 0 {
                                    first_rooted = slot;
                                }
                                last_rooted = slot;
                                total_rooted += 1;
                            }
                            let mut count_past_root = 0;
                            for slot in slots.iter().rev() {
                                if *slot > last_rooted {
                                    count_past_root += 1;
                                } else {
                                    break;
                                }
                            }
                            println!(
                                "  with {} rooted slots from {:?} to {:?}",
                                total_rooted, first_rooted, last_rooted
                            );
                            println!("  and {} slots past the last root", count_past_root);
                        } else {
                            println!("  with no rooted slots");
                        }
                    }
                    Err(err) => {
                        eprintln!("Unable to read the Ledger: {:?}", err);
                        exit(1);
                    }
                };
            }
            ("analyze-storage", _) => {
                analyze_storage(
                    &open_blockstore(&ledger_path, AccessType::Secondary, wal_recovery_mode).db(),
                );
                println!("Ok.");
            }
            ("compute-slot-cost", Some(arg_matches)) => {
                let blockstore =
                    open_blockstore(&ledger_path, AccessType::Secondary, wal_recovery_mode);

                let mut slots: Vec<u64> = vec![];
                if !arg_matches.is_present("slots") {
                    if let Ok(metas) = blockstore.slot_meta_iterator(0) {
                        slots = metas.map(|(slot, _)| slot).collect();
                    }
                } else {
                    slots = values_t_or_exit!(arg_matches, "slots", Slot);
                }

                for slot in slots {
                    if let Err(err) = compute_slot_cost(&blockstore, slot) {
                        eprintln!("{}", err);
                    }
                }
            }
            ("", _) => {
                eprintln!("{}", matches.usage());
                exit(1);
            }
            _ => unreachable!(),
        };
    }
}
