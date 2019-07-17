use clap::{crate_description, crate_name, crate_version, value_t, App, Arg, SubCommand};
use solana::blocktree::Blocktree;
use solana::blocktree_processor::process_blocktree;
use solana_sdk::genesis_block::GenesisBlock;
use std::collections::BTreeMap;
use std::fs::File;
use std::io::{stdout, Write};
use std::process::exit;
use std::str::FromStr;

#[derive(PartialEq)]
enum LedgerOutputMethod {
    Print,
    Json,
}
fn output_ledger(blocktree: Blocktree, starting_slot: u64, method: LedgerOutputMethod) {
    let rooted_slot_iterator = blocktree
        .rooted_slot_iterator(starting_slot)
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

    for (slot, slot_meta) in rooted_slot_iterator {
        match method {
            LedgerOutputMethod::Print => println!("Slot {}", slot),
            LedgerOutputMethod::Json => {
                serde_json::to_writer(stdout(), &slot_meta).expect("serialize slot_meta");
                stdout().write_all(b",\n").expect("newline");
            }
        }

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

    if method == LedgerOutputMethod::Json {
        stdout().write_all(b"\n]}\n").expect("close array");
    }
}

fn main() {
    const DEFAULT_ROOT_COUNT: &str = "1";
    solana_logger::setup();
    let matches = App::new(crate_name!())
        .about(crate_description!())
        .version(crate_version!())
        .arg(
            Arg::with_name("ledger")
                .short("l")
                .long("ledger")
                .value_name("DIR")
                .takes_value(true)
                .required(true)
                .help("Use directory for ledger location"),
        )
        .arg(
            Arg::with_name("starting_slot")
                .long("starting-slot")
                .value_name("NUM")
                .takes_value(true)
                .default_value("0")
                .help("Start at this slot (only applies to print and json commands)"),
        )
        .subcommand(SubCommand::with_name("print").about("Print the ledger"))
        .subcommand(SubCommand::with_name("json").about("Print the ledger in JSON format"))
        .subcommand(SubCommand::with_name("verify").about("Verify the ledger's PoH"))
        .subcommand(SubCommand::with_name("prune").about("Prune the ledger at the block height").arg(
            Arg::with_name("slot_list")
                .long("slot-list")
                .value_name("FILENAME")
                .takes_value(true)
                .required(true)
                .help("The location of the YAML file with a list of rollback slot heights and hashes"),
        ))
        .subcommand(SubCommand::with_name("list-roots").about("Output upto last <num-roots> root hashes and their heights starting at the given block height").arg(
            Arg::with_name("max_height")
                .long("max-height")
                .value_name("NUM")
                .takes_value(true)
                .required(true)
                .help("Maximum block height"),
        ).arg(
            Arg::with_name("slot_list")
                .long("slot-list")
                .value_name("FILENAME")
                .required(false)
                .takes_value(true)
                .help("The location of the output YAML file. A list of rollback slot heights and hashes will be written to the file."),
        ).arg(
            Arg::with_name("num_roots")
                .long("num-roots")
                .value_name("NUM")
                .takes_value(true)
                .default_value(DEFAULT_ROOT_COUNT)
                .required(false)
                .help("Number of roots in the output"),
        ))
        .get_matches();

    let ledger_path = matches.value_of("ledger").unwrap();

    let genesis_block = GenesisBlock::load(ledger_path).unwrap_or_else(|err| {
        eprintln!(
            "Failed to open ledger genesis_block at {}: {}",
            ledger_path, err
        );
        exit(1);
    });

    let blocktree = match Blocktree::open(ledger_path) {
        Ok(blocktree) => blocktree,
        Err(err) => {
            eprintln!("Failed to open ledger at {}: {}", ledger_path, err);
            exit(1);
        }
    };

    let starting_slot = value_t!(matches, "starting_slot", u64).unwrap_or_else(|e| e.exit());

    match matches.subcommand() {
        ("print", _) => {
            output_ledger(blocktree, starting_slot, LedgerOutputMethod::Print);
        }
        ("json", _) => {
            output_ledger(blocktree, starting_slot, LedgerOutputMethod::Json);
        }
        ("verify", _) => {
            println!("Verifying ledger...");
            match process_blocktree(&genesis_block, &blocktree, None, true) {
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

                let iter = blocktree
                    .rooted_slot_iterator(0)
                    .expect("Failed to get rooted slot");

                let potential_hashes: Vec<_> = iter
                    .filter_map(|(slot, meta)| {
                        let blockhash = blocktree
                            .get_slot_entries(slot, meta.last_index, Some(1))
                            .unwrap()
                            .first()
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

            let iter = blocktree
                .rooted_slot_iterator(0)
                .expect("Failed to get rooted slot");

            let slot_hash: Vec<_> = iter
                .filter_map(|(slot, meta)| {
                    if slot <= max_height as u64 {
                        let blockhash = blocktree
                            .get_slot_entries(slot, meta.last_index, Some(1))
                            .unwrap()
                            .first()
                            .unwrap()
                            .hash;
                        Some((slot, blockhash))
                    } else {
                        None
                    }
                })
                .collect();

            let mut output_file: Box<Write> = if let Some(path) = args_matches.value_of("slot_list")
            {
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
        ("", _) => {
            eprintln!("{}", matches.usage());
            exit(1);
        }
        _ => unreachable!(),
    };
}
