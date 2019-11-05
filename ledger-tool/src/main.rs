use clap::{
    crate_description, crate_name, crate_version, value_t, value_t_or_exit, App, Arg, SubCommand,
};
use solana_ledger::{
    bank_forks::{BankForks, SnapshotConfig},
    bank_forks_utils,
    blocktree::Blocktree,
    blocktree_processor,
    rooted_slot_iterator::RootedSlotIterator,
};
use solana_sdk::{clock::Slot, genesis_block::GenesisBlock, native_token::lamports_to_sol};
use solana_vote_api::vote_state::VoteState;
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fs::File,
    io::{stdout, Write},
    path::PathBuf,
    process::exit,
    str::FromStr,
};

#[derive(PartialEq)]
enum LedgerOutputMethod {
    Print,
    Json,
}

fn output_slot(blocktree: &Blocktree, slot: Slot, method: &LedgerOutputMethod) {
    let entries = blocktree
        .get_slot_entries(slot, 0, None)
        .unwrap_or_else(|err| {
            eprintln!("Failed to load entries for slot {}: {:?}", slot, err);
            exit(1);
        });

    for entry in entries {
        match method {
            LedgerOutputMethod::Print => println!("{:?}", entry),
            LedgerOutputMethod::Json => {
                serde_json::to_writer(stdout(), &entry).expect("serialize entry");
                stdout().write_all(b",\n").expect("newline");
            }
        }
    }
}

fn output_ledger(blocktree: Blocktree, starting_slot: Slot, method: LedgerOutputMethod) {
    let rooted_slot_iterator =
        RootedSlotIterator::new(starting_slot, &blocktree).unwrap_or_else(|err| {
            eprintln!(
                "Failed to load entries starting from slot {}: {:?}",
                starting_slot, err
            );
            exit(1);
        });

    if method == LedgerOutputMethod::Json {
        stdout().write_all(b"{\"ledger\":[\n").expect("open array");
    }

    for (slot, slot_meta) in rooted_slot_iterator {
        match method {
            LedgerOutputMethod::Print => println!("Slot {}", slot),
            LedgerOutputMethod::Json => {
                serde_json::to_writer(stdout(), &slot_meta).expect("serialize slot_meta");
                stdout().write_all(b",\n").expect("newline");
            }
        }

        output_slot(&blocktree, slot, &method);
    }

    if method == LedgerOutputMethod::Json {
        stdout().write_all(b"\n]}\n").expect("close array");
    }
}

fn graph_forks(
    dot_file: &str,
    bank_forks: BankForks,
    bank_forks_info: Vec<blocktree_processor::BankForksInfo>,
) {
    // Search all forks and collect the last vote made by each validator
    let mut last_votes = HashMap::new();
    for bfi in &bank_forks_info {
        let bank = bank_forks.banks.get(&bfi.bank_slot).unwrap();

        let total_stake = bank
            .vote_accounts()
            .iter()
            .fold(0, |acc, (_, (stake, _))| acc + stake);
        for (_, (stake, vote_account)) in bank.vote_accounts() {
            let vote_state = VoteState::from(&vote_account).unwrap_or_default();
            if let Some(last_vote) = vote_state.votes.iter().last() {
                let entry = last_votes.entry(vote_state.node_pubkey).or_insert((
                    last_vote.slot,
                    vote_state.clone(),
                    stake,
                    total_stake,
                ));
                if entry.0 < last_vote.slot {
                    *entry = (last_vote.slot, vote_state, stake, total_stake);
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
    for bfi in &bank_forks_info {
        let bank = bank_forks.banks.get(&bfi.bank_slot).unwrap();
        let mut bank = bank.clone();

        let mut first = true;
        loop {
            if !styled_slots.contains(&bank.slot()) {
                dot.push(format!(
                    r#"    "{}"[label="{} (epoch {})\nleader: {}{}",style="{}{}"];"#,
                    bank.slot(),
                    bank.slot(),
                    bank.epoch(),
                    bank.collector_id(),
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
                    if !bank.is_votable() { "dotted," } else { "" }
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
        dot.push(format!(
            r#"  "{}"[shape=box,label="validator: {}\nstake: {} SOL\nroot slot: {}\nlatest votes:\n{}"];"#,
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
            r#"  "{}" -> "{}" [style=dotted,label="last vote"];"#,
            node_pubkey,
            if styled_slots.contains(&last_vote_slot) {
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

    dot.push("}".to_string());

    match File::create(dot_file).and_then(|mut file| file.write_all(&dot.join("\n").into_bytes())) {
        Ok(_) => println!("Wrote {}", dot_file),
        Err(err) => eprintln!("Unable to write {}: {}", dot_file, err),
    };
}

fn main() {
    const DEFAULT_ROOT_COUNT: &str = "1";
    solana_logger::setup_with_filter("solana=info");

    let starting_slot_arg = Arg::with_name("starting_slot")
        .long("starting-slot")
        .value_name("NUM")
        .takes_value(true)
        .default_value("0")
        .help("Start at this slot");

    let matches = App::new(crate_name!())
        .about(crate_description!())
        .version(crate_version!())
        .arg(
            Arg::with_name("ledger")
                .short("l")
                .long("ledger")
                .value_name("DIR")
                .takes_value(true)
                .global(true)
                .help("Use directory for ledger location"),
        )
        .subcommand(
            SubCommand::with_name("print")
            .about("Print the ledger")
            .arg(&starting_slot_arg)
        )
        .subcommand(
            SubCommand::with_name("print-slot")
            .about("Print the contents of one slot")
            .arg(
                Arg::with_name("slot")
                    .index(1)
                    .value_name("SLOT")
                    .takes_value(true)
                    .required(true)
                    .help("The slot to print"),
            )
        )
        .subcommand(
            SubCommand::with_name("bounds")
            .about("Print lowest and highest non-empty slots. Note: This ignores gaps in slots")
        )
        .subcommand(
            SubCommand::with_name("json")
            .about("Print the ledger in JSON format")
            .arg(&starting_slot_arg)
        )
        .subcommand(
            SubCommand::with_name("verify")
            .about("Verify the ledger")
            .arg(
                Arg::with_name("no_snapshot")
                    .long("no-snapshot")
                    .takes_value(false)
                    .help("Do not start from a local snapshot if present"),
            )
            .arg(
                Arg::with_name("account_paths")
                    .long("accounts")
                    .value_name("PATHS")
                    .takes_value(true)
                    .help("Comma separated persistent accounts location"),
            )
            .arg(
                Arg::with_name("halt_at_slot")
                    .long("halt-at-slot")
                    .value_name("SLOT")
                    .takes_value(true)
                    .help("Halt processing at the given slot"),
            )
            .arg(
                Arg::with_name("skip_poh_verify")
                    .long("skip-poh-verify")
                    .takes_value(false)
                    .help("Skip ledger PoH verification"),
            )
            .arg(
                Arg::with_name("graph_forks")
                    .long("graph-forks")
                    .value_name("FILENAME.GV")
                    .takes_value(true)
                    .help("Create a Graphviz DOT file representing the active forks once the ledger is verified"),
            )
        ).subcommand(
            SubCommand::with_name("prune")
            .about("Prune the ledger at the block height")
            .arg(
                Arg::with_name("slot_list")
                    .long("slot-list")
                    .value_name("FILENAME")
                    .takes_value(true)
                    .required(true)
                    .help("The location of the YAML file with a list of rollback slot heights and hashes"),
            )
        )
        .subcommand(
            SubCommand::with_name("list-roots")
            .about("Output upto last <num-roots> root hashes and their heights starting at the given block height")
            .arg(
                Arg::with_name("max_height")
                    .long("max-height")
                    .value_name("NUM")
                    .takes_value(true)
                    .required(true)
                    .help("Maximum block height")
            )
            .arg(
                Arg::with_name("slot_list")
                    .long("slot-list")
                    .value_name("FILENAME")
                    .required(false)
                    .takes_value(true)
                    .help("The location of the output YAML file. A list of rollback slot heights and hashes will be written to the file.")
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
        .get_matches();

    let ledger_path = PathBuf::from(value_t_or_exit!(matches, "ledger", String));

    let genesis_block = GenesisBlock::load(&ledger_path).unwrap_or_else(|err| {
        eprintln!(
            "Failed to open ledger genesis_block at {:?}: {}",
            ledger_path, err
        );
        exit(1);
    });

    let blocktree = match Blocktree::open(&ledger_path) {
        Ok(blocktree) => blocktree,
        Err(err) => {
            eprintln!("Failed to open ledger at {:?}: {:?}", ledger_path, err);
            exit(1);
        }
    };

    match matches.subcommand() {
        ("print", Some(args_matches)) => {
            let starting_slot = value_t_or_exit!(args_matches, "starting_slot", Slot);
            output_ledger(blocktree, starting_slot, LedgerOutputMethod::Print);
        }
        ("print-slot", Some(args_matches)) => {
            let slot = value_t_or_exit!(args_matches, "slot", Slot);
            output_slot(&blocktree, slot, &LedgerOutputMethod::Print);
        }
        ("json", Some(args_matches)) => {
            let starting_slot = value_t_or_exit!(args_matches, "starting_slot", Slot);
            output_ledger(blocktree, starting_slot, LedgerOutputMethod::Json);
        }
        ("verify", Some(arg_matches)) => {
            println!("Verifying ledger...");

            let dev_halt_at_slot = value_t!(arg_matches, "halt_at_slot", Slot).ok();
            let poh_verify = !arg_matches.is_present("skip_poh_verify");

            let snapshot_config = if arg_matches.is_present("no_snapshot") {
                None
            } else {
                Some(SnapshotConfig {
                    snapshot_interval_slots: 0, // Value doesn't matter
                    snapshot_package_output_path: ledger_path.clone(),
                    snapshot_path: ledger_path.clone().join("snapshot"),
                })
            };
            let account_paths = if let Some(account_paths) = matches.value_of("account_paths") {
                Some(account_paths.to_string())
            } else {
                Some(ledger_path.join("accounts").to_str().unwrap().to_string())
            };

            let process_options = blocktree_processor::ProcessOptions {
                poh_verify,
                dev_halt_at_slot,
                ..blocktree_processor::ProcessOptions::default()
            };

            match bank_forks_utils::load(
                &genesis_block,
                &blocktree,
                account_paths,
                snapshot_config.as_ref(),
                process_options,
            ) {
                Ok((bank_forks, bank_forks_info, _leader_schedule_cache)) => {
                    println!("Ok");

                    if let Some(dot_file) = arg_matches.value_of("graph_forks") {
                        graph_forks(&dot_file, bank_forks, bank_forks_info);
                    }
                }
                Err(err) => {
                    eprintln!("Ledger verification failed: {:?}", err);
                    exit(1);
                }
            }
        }
        ("prune", Some(args_matches)) => {
            if let Some(prune_file_path) = args_matches.value_of("slot_list") {
                let prune_file = File::open(prune_file_path.to_string()).unwrap();
                let slot_hashes: BTreeMap<u64, String> =
                    serde_yaml::from_reader(prune_file).unwrap();

                let iter =
                    RootedSlotIterator::new(0, &blocktree).expect("Failed to get rooted slot");

                let potential_hashes: Vec<_> = iter
                    .filter_map(|(slot, _meta)| {
                        let blockhash = blocktree
                            .get_slot_entries(slot, 0, None)
                            .unwrap()
                            .last()
                            .unwrap()
                            .hash
                            .to_string();

                        slot_hashes.get(&slot).and_then(|hash| {
                            if *hash == blockhash {
                                Some((slot, blockhash))
                            } else {
                                None
                            }
                        })
                    })
                    .collect();

                let (target_slot, target_hash) = potential_hashes
                    .last()
                    .expect("Failed to find a valid slot");
                println!("Prune at slot {:?} hash {:?}", target_slot, target_hash);
                blocktree.prune(*target_slot);
            }
        }
        ("list-roots", Some(args_matches)) => {
            let max_height = if let Some(height) = args_matches.value_of("max_height") {
                usize::from_str(height).expect("Maximum height must be a number")
            } else {
                panic!("Maximum height must be provided");
            };
            let num_roots = if let Some(roots) = args_matches.value_of("num_roots") {
                usize::from_str(roots).expect("Number of roots must be a number")
            } else {
                usize::from_str(DEFAULT_ROOT_COUNT).unwrap()
            };

            let iter = RootedSlotIterator::new(0, &blocktree).expect("Failed to get rooted slot");

            let slot_hash: Vec<_> = iter
                .filter_map(|(slot, _meta)| {
                    if slot <= max_height as u64 {
                        let blockhash = blocktree
                            .get_slot_entries(slot, 0, None)
                            .unwrap()
                            .last()
                            .unwrap()
                            .hash;
                        Some((slot, blockhash))
                    } else {
                        None
                    }
                })
                .collect();

            let mut output_file: Box<dyn Write> =
                if let Some(path) = args_matches.value_of("slot_list") {
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
        ("bounds", _) => match blocktree.slot_meta_iterator(0) {
            Ok(metas) => {
                println!("Collecting Ledger information...");
                let slots: Vec<_> = metas.map(|(slot, _)| slot).collect();
                if slots.is_empty() {
                    println!("Ledger is empty. No slots found.");
                } else {
                    let first = slots.first().unwrap();
                    let last = slots.last().unwrap_or_else(|| first);
                    if first != last {
                        println!(
                            "Ledger contains some data for slots {:?} to {:?}",
                            first, last
                        );
                    } else {
                        println!("Ledger only contains some data for slot {:?}", first);
                    }
                }
            }
            Err(err) => {
                eprintln!("Unable to read the Ledger: {:?}", err);
                exit(1);
            }
        },
        ("", _) => {
            eprintln!("{}", matches.usage());
            exit(1);
        }
        _ => unreachable!(),
    };
}
