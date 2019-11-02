use clap::{crate_description, crate_name, crate_version, value_t_or_exit, App, Arg, SubCommand};
use solana_ledger::blocktree::Blocktree;
use solana_ledger::blocktree_processor::{process_blocktree, ProcessOptions};
use solana_ledger::rooted_slot_iterator::RootedSlotIterator;
use solana_sdk::clock::Slot;
use solana_sdk::genesis_block::GenesisBlock;
use std::collections::BTreeMap;
use std::fs::File;
use std::io::{stdout, Write};
use std::path::PathBuf;
use std::process::exit;
use std::str::FromStr;

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

fn main() {
    const DEFAULT_ROOT_COUNT: &str = "1";
    solana_logger::setup();

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
        .subcommand(SubCommand::with_name("print").about("Print the ledger").arg(&starting_slot_arg))
        .subcommand(SubCommand::with_name("print-slot").about("Print the contents of one slot").arg(
            Arg::with_name("slot")
                .index(1)
                .value_name("SLOT")
                .takes_value(true)
                .required(true)
                .help("The slot to print"),
        ))
        .subcommand(SubCommand::with_name("bounds").about("Print lowest and highest non-empty slots. Note: This ignores gaps in slots"))
        .subcommand(SubCommand::with_name("json").about("Print the ledger in JSON format").arg(&starting_slot_arg))
        .subcommand(SubCommand::with_name("verify").about("Verify the ledger's PoH"))
        .subcommand(SubCommand::with_name("prune").about("Prune the ledger at the block height").arg(
            Arg::with_name("slot_list")
                .long("slot-list")
                .value_name("FILENAME")
                .takes_value(true)
                .required(true)
                .help("The location of the YAML file with a list of rollback slot heights and hashes"),
        ))
        .subcommand(
            SubCommand::with_name("list-roots")
            .about("Output upto last <num-roots> root hashes and their heights starting at the given block height")
            .arg(
                Arg::with_name("max_height")
                    .long("max-height")
                    .value_name("NUM")
                    .takes_value(true)
                    .required(true)
                    .help("Maximum block height")).arg(
            Arg::with_name("slot_list")
                .long("slot-list")
                .value_name("FILENAME")
                .required(false)
                .takes_value(true)
                .help("The location of the output YAML file. A list of rollback slot heights and hashes will be written to the file.")).arg(
            Arg::with_name("num_roots")
                .long("num-roots")
                .value_name("NUM")
                .takes_value(true)
                .default_value(DEFAULT_ROOT_COUNT)
                .required(false)
                .help("Number of roots in the output"),
        ))
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
        ("verify", _) => {
            println!("Verifying ledger...");
            let options = ProcessOptions {
                verify_ledger: true,
                ..ProcessOptions::default()
            };
            match process_blocktree(&genesis_block, &blocktree, None, options) {
                Ok((_bank_forks, bank_forks_info, _)) => {
                    println!("{:?}", bank_forks_info);
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
