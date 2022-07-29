#![allow(clippy::integer_arithmetic)]
use {
    crate::{bigtable::*, ledger_path::*},
    chrono::{DateTime, Utc},
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
        blockstore_db::{self, Database},
        blockstore_options::{
            AccessType, BlockstoreOptions, BlockstoreRecoveryMode, LedgerColumnOptions,
            ShredStorageType,
        },
        blockstore_processor::{BlockstoreProcessorError, ProcessOptions},
        shred::Shred,
    },
    solana_measure::{measure, measure::Measure},
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
        snapshot_minimizer::SnapshotMinimizer,
        snapshot_utils::{
            self, ArchiveFormat, SnapshotVersion, DEFAULT_ARCHIVE_COMPRESSION,
            DEFAULT_MAX_FULL_SNAPSHOT_ARCHIVES_TO_RETAIN,
            DEFAULT_MAX_INCREMENTAL_SNAPSHOT_ARCHIVES_TO_RETAIN, SUPPORTED_ARCHIVE_COMPRESSION,
        },
    },
    solana_scheduler::{AddressBook, ScheduleStage, TaskQueue, Weight},
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
        time::{Duration, UNIX_EPOCH},
    },
};

mod bigtable;
mod ledger_path;

#[derive(PartialEq, Eq)]
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
    to_schedule_stage: &mut Vec<Box<SanitizedTransaction>>,
) {
    match method {
        LedgerOutputMethod::Print => {
            /*
            println!(
                "  Entry {} - num_hashes: {}, hash: {}, transactions: {}",
                entry_index,
                entry.num_hashes,
                entry.hash,
                entry.transactions.len()
            );
            */
            for (transactions_index, transaction) in entry.transactions.into_iter().enumerate() {
                //println!("    Transaction {}", transactions_index);
                let sanitized_tx = SanitizedTransaction::try_create(
                    transaction.clone(),
                    MessageHash::Compute,
                    None,
                    SimpleAddressLoader::Disabled,
                    true, // require_static_program_ids
                )
                .unwrap();
                to_schedule_stage.push(Box::new(sanitized_tx));
                /*
                let tx_signature = transaction.signatures[0];
                let tx_status_meta = blockstore
                    .read_transaction_status((tx_signature, slot))
                    .unwrap_or_else(|err| {
                        eprintln!(
                            "Failed to read transaction status for {} at slot {}: {}",
                            transaction.signatures[0], slot, err
                        );
                        None
                    })
                    .map(|meta| meta.into());

                solana_cli_output::display::println_transaction(
                    &transaction,
                    tx_status_meta.as_ref(),
                    "      ",
                    None,
                    None,
                );
                */
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
    all_program_ids: &mut HashMap<Pubkey, u64>,
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

    // basically, this controls the maximum size of task queue, so unbounded isn't nice
    let (tx_sender, tx_receiver) = crossbeam_channel::unbounded()

    // this should be target number of saturated cpu cores
    let lane_count = std::env::var("EXECUTION_LANE_COUNT")
        .unwrap_or(format!("{}", std::thread::available_parallelism().unwrap()))
        .parse::<usize>()
        .unwrap();
    let lane_channel_factor = std::env::var("LANE_CHANNEL_FACTOR")
        .unwrap_or(format!("{}", std::thread::available_parallelism().unwrap()))
        .parse::<usize>()
        .unwrap();
    //let (pre_execute_env_sender, pre_execute_env_receiver) = crossbeam_channel::bounded(lane_count * lane_channel_factor);
    let (pre_execute_env_sender, pre_execute_env_receiver) = crossbeam_channel::unbounded();


    //let (pre_execute_env_sender, pre_execute_env_receiver) = crossbeam_channel::unbounded();
    //let (post_execute_env_sender, post_execute_env_receiver) = crossbeam_channel::unbounded();
    //
    let (post_schedule_env_sender, post_schedule_env_receiver) = crossbeam_channel::unbounded();
    let mut runnable_queue = TaskQueue::default();
    let mut contended_queue = TaskQueue::default();
    let mut address_book = AddressBook::default();
    let t1 = std::thread::Builder::new()
        .name("sol-scheduler".to_string())
        .spawn(move || loop {
            ScheduleStage::run(
                lane_count * lane_channel_factor,
                &mut runnable_queue,
                &mut contended_queue,
                &mut address_book,
                &tx_receiver,
                &pre_execute_env_sender,
                &post_schedule_env_sender,
            );
        })
        .unwrap();
    let handles = (0..lane_count)
        .map(|thx| {
            let pre_execute_env_receiver = pre_execute_env_receiver.clone();
            let post_execute_env_sender = post_execute_env_sender.clone();
            let t2 = std::thread::Builder::new()
                .name(format!("blockstore_processor_{}", thx))
                .spawn(move || {
                    use solana_metrics::datapoint_info;
                    let current_thread_name = std::thread::current().name().unwrap().to_string();

                    for step in 0.. {
                        let ee = pre_execute_env_receiver.recv().unwrap();

                        let mut process_message_time = Measure::start("process_message_time");

                        let sig = ee.task.tx.signature().to_string();
                        trace!("execute substage: #{} {:#?}", step, &sig);
                        std::thread::sleep(std::time::Duration::from_micros(ee.cu.try_into().unwrap()));

                        process_message_time.stop();
                        let duration_with_overhead = process_message_time.as_us();

                        datapoint_info!(
                            "individual_tx_stats",
                            ("slot", 33333, i64),
                            ("thread", current_thread_name, String),
                            ("signature", &sig, String),
                            ("account_locks_in_json", "{}", String),
                            ("status", "Ok", String),
                            ("duration", duration_with_overhead, i64),
                            ("compute_units", ee.cu, i64),
                        );

                        post_execute_env_sender.send(Incoming::FromExecute(ee)).unwrap();
                    }
                })
                .unwrap();
            t2
        })
        .collect::<Vec<_>>();

    let depth = std::atomics::AtomicUsize::default();

    let t3 = std::thread::Builder::new()
        .name("sol-consumer".to_string())
        .spawn(move || {
            for step in 0.. {
                let ee = post_schedule_env_receiver.recv().unwrap();
                depth.fetch_sub(1, Ordering::Relaxed);
                trace!(
                    "post schedule stage: #{} {:#?}",
                    step,
                    ee.task.tx.signature()
                );
                if step % 1966 == 0 {
                    error!("finished!: {}", step);
                }
            }
        })
        .unwrap();

    if verbose_level >= 2 {
        let mut txes = Vec::new();
        for (entry_index, entry) in entries.into_iter().enumerate() {
            output_entry(blockstore, method, slot, entry_index, entry, &mut txes);
        }

        let mut weight = 10_000_000;
        for i in 0..1000 {
            error!("started!: {}", i);
            for tx in txes.clone() {
                if depth.load(Ordering::Relaxed) < 10_000 {
                    tx_sender.send(Incoming::FromPrevious((Weight { ix: weight }, tx))).unwrap();
                    depth.fetch_add(1, Ordering::Relaxed);
                    weight -= 1;
                } else {
                    std::thread::sleep(std::time::Duration::from_micros(10));
                }
            }
        }
        t1.join().unwrap();
        handles.into_iter().for_each(|t| t.join().unwrap());
        t3.join().unwrap();

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
        for (pubkey, count) in program_ids.iter() {
            *all_program_ids.entry(*pubkey).or_insert(0) += count;
        }
        println!("  Programs:");
        output_sorted_program_ids(program_ids);
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
    let mut all_program_ids = HashMap::new();
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

        if let Err(err) = output_slot(
            &blockstore,
            slot,
            allow_dead_slots,
            &method,
            verbose_level,
            &mut all_program_ids,
        ) {
            eprintln!("{}", err);
        }
        num_printed += 1;
        if num_printed >= num_slots as usize {
            break;
        }
    }

    if method == LedgerOutputMethod::Json {
        stdout().write_all(b"\n]}\n").expect("close array");
    } else {
        println!("Summary of Programs:");
        output_sorted_program_ids(all_program_ids);
    }
}

fn output_sorted_program_ids(program_ids: HashMap<Pubkey, u64>) {
    let mut program_ids_array: Vec<_> = program_ids.into_iter().collect();
    // Sort descending by count of program id
    program_ids_array.sort_by(|a, b| b.1.cmp(&a.1));
    for (program_id, count) in program_ids_array.iter() {
        println!("{:<44}: {}", program_id.to_string(), count);
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
    analyze_column::<OptimisticSlots>(database, "OptimisticSlots");
}

fn open_blockstore(
    ledger_path: &Path,
    access_type: AccessType,
    wal_recovery_mode: Option<BlockstoreRecoveryMode>,
    shred_storage_type: &ShredStorageType,
) -> Blockstore {
    match Blockstore::open_with_options(
        ledger_path,
        BlockstoreOptions {
            access_type,
            recovery_mode: wal_recovery_mode,
            enforce_ulimit_nofile: true,
            column_options: LedgerColumnOptions {
                shred_storage_type: shred_storage_type.clone(),
                ..LedgerColumnOptions::default()
            },
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
    incremental_snapshot_archive_path: Option<PathBuf>,
) -> Result<(Arc<RwLock<BankForks>>, Option<StartingSnapshotHashes>), BlockstoreProcessorError> {
    let bank_snapshots_dir = blockstore
        .ledger_path()
        .join(if blockstore.is_primary_access() {
            "snapshot"
        } else {
            "snapshot.ledger-tool"
        });

    let mut starting_slot = 0; // default start check with genesis
    let snapshot_config = if arg_matches.is_present("no_snapshot") {
        None
    } else {
        let full_snapshot_archives_dir =
            snapshot_archive_path.unwrap_or_else(|| blockstore.ledger_path().to_path_buf());
        let incremental_snapshot_archives_dir =
            incremental_snapshot_archive_path.unwrap_or_else(|| full_snapshot_archives_dir.clone());
        if let Some(full_snapshot_slot) =
            snapshot_utils::get_highest_full_snapshot_archive_slot(&full_snapshot_archives_dir)
        {
            let incremental_snapshot_slot =
                snapshot_utils::get_highest_incremental_snapshot_archive_slot(
                    &incremental_snapshot_archives_dir,
                    full_snapshot_slot,
                )
                .unwrap_or_default();
            starting_slot = std::cmp::max(full_snapshot_slot, incremental_snapshot_slot);
        }

        Some(SnapshotConfig {
            full_snapshot_archive_interval_slots: Slot::MAX,
            incremental_snapshot_archive_interval_slots: Slot::MAX,
            full_snapshot_archives_dir,
            incremental_snapshot_archives_dir,
            bank_snapshots_dir,
            ..SnapshotConfig::default()
        })
    };

    if let Some(halt_slot) = process_options.halt_at_slot {
        // Check if we have the slot data necessary to replay from starting_slot to halt_slot.
        //  - This will not catch the case when loading from genesis without a full slot 0.
        if !blockstore.slots_connected(starting_slot, halt_slot) {
            eprintln!(
                "Unable to load bank forks at slot {} due to disconnected blocks.",
                halt_slot,
            );
            exit(1);
        }
    }

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

/// Finds the accounts needed to replay slots `snapshot_slot` to `ending_slot`.
/// Removes all other accounts from accounts_db, and updates the accounts hash
/// and capitalization. This is used by the --minimize option in create-snapshot
fn minimize_bank_for_snapshot(
    blockstore: &Blockstore,
    bank: &Bank,
    snapshot_slot: Slot,
    ending_slot: Slot,
) {
    let (transaction_account_set, transaction_accounts_measure) = measure!(
        blockstore.get_accounts_used_in_range(snapshot_slot, ending_slot),
        "get transaction accounts"
    );
    let total_accounts_len = transaction_account_set.len();
    info!("Added {total_accounts_len} accounts from transactions. {transaction_accounts_measure}");

    SnapshotMinimizer::minimize(bank, snapshot_slot, ending_slot, transaction_account_set);
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

/// The default size for data and coding shred column families in FIFO compaction.
/// u64::MAX as the default value means it won't delete any files by default.
const DEFAULT_LEDGER_TOOL_ROCKS_FIFO_SHRED_STORAGE_SIZE_BYTES: u64 = std::u64::MAX;

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
    const DEFAULT_LATEST_OPTIMISTIC_SLOTS_COUNT: &str = "1";
    const DEFAULT_MAX_SLOTS_ROOT_REPAIR: &str = "2000";
    solana_logger::setup_with_default("solana=info");

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
        .help("How much memory the accounts index can consume. If this is exceeded, some account index entries will be stored on disk.");
    let disable_disk_index = Arg::with_name("disable_accounts_disk_index")
        .long("disable-accounts-disk-index")
        .help("Disable the disk-based accounts index. It is enabled by default. The entire accounts index will be kept in memory.")
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
    let accounts_db_skip_initial_hash_calc_arg =
        Arg::with_name("accounts_db_skip_initial_hash_calculation")
            .long("accounts-db-skip-initial-hash-calculation")
            .help("Do not verify accounts hash at startup.")
            .hidden(true);
    let ancient_append_vecs = Arg::with_name("accounts_db_ancient_append_vecs")
        .long("accounts-db-ancient-append-vecs")
        .help("AppendVecs that are older than an epoch are squashed together.")
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
    let debug_key_arg = Arg::with_name("debug_key")
        .long("debug-key")
        .validator(is_pubkey)
        .value_name("ADDRESS")
        .multiple(true)
        .takes_value(true)
        .help("Log when transactions are processed that reference the given key(s).");

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

    let mut measure_total_execution_time = Measure::start("ledger tool");
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
                .help("Use DIR for snapshot location"),
        )
        .arg(
            Arg::with_name("incremental_snapshot_archive_path")
                .long("incremental-snapshot-archive-path")
                .value_name("DIR")
                .takes_value(true)
                .global(true)
                .help("Use DIR for separate incremental snapshot location"),
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
            .arg(&accounts_db_skip_initial_hash_calc_arg)
            .arg(&ancient_append_vecs)
            .arg(&hard_forks_arg)
            .arg(&no_accounts_db_caching_arg)
            .arg(&accounts_db_test_hash_calculation_arg)
            .arg(&no_os_memory_stats_reporting_arg)
            .arg(&no_bpf_jit_arg)
            .arg(&allow_dead_slots_arg)
            .arg(&max_genesis_archive_unpacked_size_arg)
            .arg(&debug_key_arg)
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
            .arg(&skip_rewrites_arg)
            .arg(&accounts_db_skip_initial_hash_calc_arg)
            .arg(&ancient_append_vecs)
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
            .arg(
                Arg::with_name("minimized")
                    .long("minimized")
                    .takes_value(false)
                    .help("Create a minimized snapshot instead of a full snapshot. This snapshot \
                          will only include information needed to replay the ledger from the \
                          snapshot slot to the ending slot.")
                    .conflicts_with("incremental")
                    .requires("ending_slot")
            )
            .arg(
                Arg::with_name("ending_slot")
                    .long("ending-slot")
                    .takes_value(true)
                    .value_name("ENDING_SLOT")
                    .help("Ending slot for minimized snapshot creation")
            )
            .arg(
                Arg::with_name("snapshot_archive_format")
                    .long("snapshot-archive-format")
                    .possible_values(SUPPORTED_ARCHIVE_COMPRESSION)
                    .default_value(DEFAULT_ARCHIVE_COMPRESSION)
                    .value_name("ARCHIVE_TYPE")
                    .takes_value(true)
                    .help("Snapshot archive format to use.")
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
            SubCommand::with_name("latest-optimistic-slots")
                .about("Output up to the most recent <num-slots> optimistic \
                        slots with their hashes and timestamps.")
                .arg(
                    Arg::with_name("num_slots")
                        .long("num-slots")
                        .value_name("NUM")
                        .takes_value(true)
                        .default_value(DEFAULT_LATEST_OPTIMISTIC_SLOTS_COUNT)
                        .required(false)
                        .help("Number of slots in the output"),
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
                        .help("Recent root after the range to repair")
                )
                .arg(
                    Arg::with_name("end_root")
                        .long("until")
                        .value_name("NUM")
                        .takes_value(true)
                        .help("Earliest slot to check for root repair")
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
    let incremental_snapshot_archive_path =
        value_t!(matches, "incremental_snapshot_archive_path", String)
            .ok()
            .map(PathBuf::from);

    let wal_recovery_mode = matches
        .value_of("wal_recovery_mode")
        .map(BlockstoreRecoveryMode::from);
    let verbose_level = matches.occurrences_of("verbose");

    // TODO: the following shred_storage_type inference must be updated once the
    // rocksdb options can be constructed via load_options_file() as the
    // temporary use of DEFAULT_LEDGER_TOOL_ROCKS_FIFO_SHRED_STORAGE_SIZE_BYTES
    // could affect the persisted rocksdb options file.
    let shred_storage_type = match ShredStorageType::from_ledger_path(
        &ledger_path,
        DEFAULT_LEDGER_TOOL_ROCKS_FIFO_SHRED_STORAGE_SIZE_BYTES,
    ) {
        Some(s) => s,
        None => {
            error!("Shred storage type cannot be inferred, the default RocksLevel will be used");
            ShredStorageType::RocksLevel
        }
    };

    if let ("bigtable", Some(arg_matches)) = matches.subcommand() {
        bigtable_process_command(&ledger_path, arg_matches, &shred_storage_type)
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
                    open_blockstore(
                        &ledger_path,
                        AccessType::Secondary,
                        wal_recovery_mode,
                        &shred_storage_type,
                    ),
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
                let source = open_blockstore(
                    &ledger_path,
                    AccessType::Secondary,
                    None,
                    &shred_storage_type,
                );
                let target =
                    open_blockstore(&target_db, AccessType::Primary, None, &shred_storage_type);
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
                let blockstore = open_blockstore(
                    &ledger_path,
                    AccessType::Secondary,
                    wal_recovery_mode,
                    &shred_storage_type,
                );
                match load_bank_forks(
                    arg_matches,
                    &genesis_config,
                    &blockstore,
                    process_options,
                    snapshot_archive_path,
                    incremental_snapshot_archive_path,
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
                let ledger = open_blockstore(
                    &ledger_path,
                    AccessType::Secondary,
                    None,
                    &shred_storage_type,
                );
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
                let blockstore = open_blockstore(
                    &ledger_path,
                    AccessType::Secondary,
                    wal_recovery_mode,
                    &shred_storage_type,
                );
                match load_bank_forks(
                    arg_matches,
                    &genesis_config,
                    &blockstore,
                    process_options,
                    snapshot_archive_path,
                    incremental_snapshot_archive_path,
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
                let blockstore = open_blockstore(
                    &ledger_path,
                    AccessType::Secondary,
                    wal_recovery_mode,
                    &shred_storage_type,
                );
                for slot in slots {
                    println!("Slot {}", slot);
                    if let Err(err) = output_slot(
                        &blockstore,
                        slot,
                        allow_dead_slots,
                        &LedgerOutputMethod::Print,
                        verbose_level,
                        &mut HashMap::new(),
                    ) {
                        eprintln!("{}", err);
                    }
                }
            }
            ("json", Some(arg_matches)) => {
                let starting_slot = value_t_or_exit!(arg_matches, "starting_slot", Slot);
                let allow_dead_slots = arg_matches.is_present("allow_dead_slots");
                output_ledger(
                    open_blockstore(
                        &ledger_path,
                        AccessType::Secondary,
                        wal_recovery_mode,
                        &shred_storage_type,
                    ),
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
                let blockstore = open_blockstore(
                    &ledger_path,
                    AccessType::Secondary,
                    wal_recovery_mode,
                    &shred_storage_type,
                );
                let starting_slot = value_t_or_exit!(arg_matches, "starting_slot", Slot);
                for slot in blockstore.dead_slots_iterator(starting_slot).unwrap() {
                    println!("{}", slot);
                }
            }
            ("duplicate-slots", Some(arg_matches)) => {
                let blockstore = open_blockstore(
                    &ledger_path,
                    AccessType::Secondary,
                    wal_recovery_mode,
                    &shred_storage_type,
                );
                let starting_slot = value_t_or_exit!(arg_matches, "starting_slot", Slot);
                for slot in blockstore.duplicate_slots_iterator(starting_slot).unwrap() {
                    println!("{}", slot);
                }
            }
            ("set-dead-slot", Some(arg_matches)) => {
                let slots = values_t_or_exit!(arg_matches, "slots", Slot);
                let blockstore = open_blockstore(
                    &ledger_path,
                    AccessType::Primary,
                    wal_recovery_mode,
                    &shred_storage_type,
                );
                for slot in slots {
                    match blockstore.set_dead_slot(slot) {
                        Ok(_) => println!("Slot {} dead", slot),
                        Err(err) => eprintln!("Failed to set slot {} dead slot: {:?}", slot, err),
                    }
                }
            }
            ("remove-dead-slot", Some(arg_matches)) => {
                let slots = values_t_or_exit!(arg_matches, "slots", Slot);
                let blockstore = open_blockstore(
                    &ledger_path,
                    AccessType::Primary,
                    wal_recovery_mode,
                    &shred_storage_type,
                );
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
                let blockstore = open_blockstore(
                    &ledger_path,
                    AccessType::Secondary,
                    wal_recovery_mode,
                    &shred_storage_type,
                );
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
                    skip_rewrites: arg_matches.is_present("accounts_db_skip_rewrites"),
                    ancient_append_vecs: arg_matches.is_present("accounts_db_ancient_append_vecs"),
                    skip_initial_hash_calc: arg_matches
                        .is_present("accounts_db_skip_initial_hash_calculation"),
                    ..AccountsDbConfig::default()
                });

                let debug_keys = pubkeys_of(arg_matches, "debug_key")
                    .map(|pubkeys| Arc::new(pubkeys.into_iter().collect::<HashSet<_>>()));

                let process_options = ProcessOptions {
                    new_hard_forks: hardforks_of(arg_matches, "hard_forks"),
                    poh_verify: !arg_matches.is_present("skip_poh_verify"),
                    halt_at_slot: value_t!(arg_matches, "halt_at_slot", Slot).ok(),
                    debug_keys,
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
                        bpf_jit: !arg_matches.is_present("no_bpf_jit"),
                        ..RuntimeConfig::default()
                    },
                    ..ProcessOptions::default()
                };
                let print_accounts_stats = arg_matches.is_present("print_accounts_stats");
                println!(
                    "genesis hash: {}",
                    open_genesis_config_by(&ledger_path, arg_matches).hash()
                );

                let blockstore = open_blockstore(
                    &ledger_path,
                    AccessType::Secondary,
                    wal_recovery_mode,
                    &shred_storage_type,
                );
                let (bank_forks, ..) = load_bank_forks(
                    arg_matches,
                    &open_genesis_config_by(&ledger_path, arg_matches),
                    &blockstore,
                    process_options,
                    snapshot_archive_path,
                    incremental_snapshot_archive_path,
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

                let blockstore = open_blockstore(
                    &ledger_path,
                    AccessType::Secondary,
                    wal_recovery_mode,
                    &shred_storage_type,
                );
                match load_bank_forks(
                    arg_matches,
                    &open_genesis_config_by(&ledger_path, arg_matches),
                    &blockstore,
                    process_options,
                    snapshot_archive_path,
                    incremental_snapshot_archive_path,
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
                let is_incremental = arg_matches.is_present("incremental");
                let is_minimized = arg_matches.is_present("minimized");
                let output_directory = value_t!(arg_matches, "output_directory", PathBuf)
                    .unwrap_or_else(|_| {
                        match (
                            is_incremental,
                            &snapshot_archive_path,
                            &incremental_snapshot_archive_path,
                        ) {
                            (true, _, Some(incremental_snapshot_archive_path)) => {
                                incremental_snapshot_archive_path.clone()
                            }
                            (_, Some(snapshot_archive_path), _) => snapshot_archive_path.clone(),
                            (_, _, _) => ledger_path.clone(),
                        }
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

                let snapshot_archive_format = {
                    let archive_format_str =
                        value_t_or_exit!(arg_matches, "snapshot_archive_format", String);
                    ArchiveFormat::from_cli_arg(&archive_format_str).unwrap_or_else(|| {
                        panic!("Archive format not recognized: {}", archive_format_str)
                    })
                };

                let maximum_full_snapshot_archives_to_retain =
                    value_t_or_exit!(arg_matches, "maximum_full_snapshots_to_retain", usize);
                let maximum_incremental_snapshot_archives_to_retain = value_t_or_exit!(
                    arg_matches,
                    "maximum_incremental_snapshots_to_retain",
                    usize
                );
                let genesis_config = open_genesis_config_by(&ledger_path, arg_matches);
                let blockstore = open_blockstore(
                    &ledger_path,
                    AccessType::Secondary,
                    wal_recovery_mode,
                    &shred_storage_type,
                );

                let snapshot_slot = if Some("ROOT") == arg_matches.value_of("snapshot_slot") {
                    blockstore
                        .rooted_slot_iterator(0)
                        .expect("Failed to get rooted slot iterator")
                        .last()
                        .expect("Failed to get root")
                } else {
                    value_t_or_exit!(arg_matches, "snapshot_slot", Slot)
                };

                let ending_slot = if is_minimized {
                    let ending_slot = value_t_or_exit!(arg_matches, "ending_slot", Slot);
                    if ending_slot <= snapshot_slot {
                        eprintln!(
                            "ending_slot ({}) must be greater than snapshot_slot ({})",
                            ending_slot, snapshot_slot
                        );
                        exit(1);
                    }

                    Some(ending_slot)
                } else {
                    None
                };

                let snapshot_type_str = if is_incremental {
                    "incremental "
                } else if is_minimized {
                    "minimized "
                } else {
                    ""
                };

                info!(
                    "Creating {}snapshot of slot {} in {}",
                    snapshot_type_str,
                    snapshot_slot,
                    output_directory.display()
                );

                let accounts_db_config = Some(AccountsDbConfig {
                    skip_rewrites: arg_matches.is_present("accounts_db_skip_rewrites"),
                    ancient_append_vecs: arg_matches.is_present("accounts_db_ancient_append_vecs"),
                    skip_initial_hash_calc: arg_matches
                        .is_present("accounts_db_skip_initial_hash_calculation"),
                    ..AccountsDbConfig::default()
                });

                match load_bank_forks(
                    arg_matches,
                    &genesis_config,
                    &blockstore,
                    ProcessOptions {
                        new_hard_forks,
                        halt_at_slot: Some(snapshot_slot),
                        poh_verify: false,
                        accounts_db_config,
                        ..ProcessOptions::default()
                    },
                    snapshot_archive_path,
                    incremental_snapshot_archive_path,
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

                        if is_minimized {
                            minimize_bank_for_snapshot(
                                &blockstore,
                                &bank,
                                snapshot_slot,
                                ending_slot.unwrap(),
                            );
                        }

                        println!(
                            "Creating a version {} {}snapshot of slot {}",
                            snapshot_version,
                            snapshot_type_str,
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
                                    output_directory.clone(),
                                    output_directory,
                                    snapshot_archive_format,
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
                                    output_directory.clone(),
                                    output_directory,
                                    snapshot_archive_format,
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

                            if is_minimized {
                                let starting_epoch = bank.epoch_schedule().get_epoch(snapshot_slot);
                                let ending_epoch =
                                    bank.epoch_schedule().get_epoch(ending_slot.unwrap());
                                if starting_epoch != ending_epoch {
                                    warn!("Minimized snapshot range crosses epoch boundary ({} to {}). Bank hashes after {} will not match replays from a full snapshot",
                                        starting_epoch, ending_epoch, bank.epoch_schedule().get_last_slot_in_epoch(starting_epoch));
                                }
                            }
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
                let blockstore = open_blockstore(
                    &ledger_path,
                    AccessType::Secondary,
                    wal_recovery_mode,
                    &shred_storage_type,
                );
                let (bank_forks, ..) = load_bank_forks(
                    arg_matches,
                    &genesis_config,
                    &blockstore,
                    process_options,
                    snapshot_archive_path,
                    incremental_snapshot_archive_path,
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
                let blockstore = open_blockstore(
                    &ledger_path,
                    AccessType::Secondary,
                    wal_recovery_mode,
                    &shred_storage_type,
                );
                match load_bank_forks(
                    arg_matches,
                    &genesis_config,
                    &blockstore,
                    process_options,
                    snapshot_archive_path,
                    incremental_snapshot_archive_path,
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
                                            use std::fmt::Write;
                                            let _ = write!(&mut detail.skipped_reasons, "/{:?}", skipped_reason);
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
                let blockstore = open_blockstore(
                    &ledger_path,
                    access_type,
                    wal_recovery_mode,
                    &shred_storage_type,
                );

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
                let blockstore = open_blockstore(
                    &ledger_path,
                    AccessType::Secondary,
                    wal_recovery_mode,
                    &shred_storage_type,
                );
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
            ("latest-optimistic-slots", Some(arg_matches)) => {
                let blockstore = open_blockstore(
                    &ledger_path,
                    AccessType::Secondary,
                    wal_recovery_mode,
                    &shred_storage_type,
                );
                let num_slots = value_t_or_exit!(arg_matches, "num_slots", usize);
                let slots = blockstore
                    .get_latest_optimistic_slots(num_slots)
                    .expect("Failed to get latest optimistic slots");
                println!("{:>20} {:>44} {:>32}", "Slot", "Hash", "Timestamp");
                for (slot, hash, timestamp) in slots.iter() {
                    let time_str = {
                        let secs: u64 = (timestamp / 1_000) as u64;
                        let nanos: u32 = ((timestamp % 1_000) * 1_000_000) as u32;
                        let t = UNIX_EPOCH + Duration::new(secs, nanos);
                        let datetime: DateTime<Utc> = t.into();
                        datetime.to_rfc3339()
                    };
                    let hash_str = format!("{}", hash);
                    println!("{:>20} {:>44} {:>32}", slot, &hash_str, &time_str);
                }
            }
            ("repair-roots", Some(arg_matches)) => {
                let blockstore = open_blockstore(
                    &ledger_path,
                    AccessType::Primary,
                    wal_recovery_mode,
                    &shred_storage_type,
                );
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
                let blockstore = open_blockstore(
                    &ledger_path,
                    AccessType::Secondary,
                    wal_recovery_mode,
                    &shred_storage_type,
                );
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
                    &open_blockstore(
                        &ledger_path,
                        AccessType::Secondary,
                        wal_recovery_mode,
                        &shred_storage_type,
                    )
                    .db(),
                );
                println!("Ok.");
            }
            ("compute-slot-cost", Some(arg_matches)) => {
                let blockstore = open_blockstore(
                    &ledger_path,
                    AccessType::Secondary,
                    wal_recovery_mode,
                    &shred_storage_type,
                );

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
        measure_total_execution_time.stop();
        info!("{}", measure_total_execution_time);
    }
}
