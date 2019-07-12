use clap::{crate_description, crate_name, crate_version, value_t, App, Arg, SubCommand};
use solana::blocktree::Blocktree;
use solana::blocktree_processor::process_blocktree;
use solana_sdk::genesis_block::GenesisBlock;
use std::io::{stdout, Write};
use std::process::exit;

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
            match process_blocktree(&genesis_block, &blocktree, None) {
                Ok((_bank_forks, bank_forks_info, _)) => {
                    println!("{:?}", bank_forks_info);
                }
                Err(err) => {
                    eprintln!("Ledger verification failed: {:?}", err);
                    exit(1);
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
