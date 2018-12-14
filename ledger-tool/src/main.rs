#[macro_use]
extern crate clap;
use serde_json;

use clap::{App, Arg, SubCommand};
use solana::bank::Bank;
use solana::ledger::{read_ledger, verify_ledger};
use solana::logger;
use std::io::{stdout, Write};
use std::process::exit;

fn main() {
    logger::setup();
    let matches = App::new("ledger-tool")
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
            Arg::with_name("head")
                .short("n")
                .long("head")
                .value_name("NUM")
                .takes_value(true)
                .help("Limit to at most the first NUM entries in ledger\n  (only applies to verify, print, json commands)"),
        )
        .arg(
            Arg::with_name("precheck")
                .short("p")
                .long("precheck")
                .help("Use ledger_verify() to check internal ledger consistency before proceeding"),
        )
        .arg(
            Arg::with_name("continue")
                .short("c")
                .long("continue")
                .help("Continue verify even if verification fails"),
        )
        .subcommand(SubCommand::with_name("print").about("Print the ledger"))
        .subcommand(SubCommand::with_name("json").about("Print the ledger in JSON format"))
        .subcommand(SubCommand::with_name("verify").about("Verify the ledger's PoH"))
        .get_matches();

    let ledger_path = matches.value_of("ledger").unwrap();

    if matches.is_present("precheck") {
        if let Err(e) = verify_ledger(&ledger_path) {
            eprintln!("ledger precheck failed, error: {:?} ", e);
            exit(1);
        }
    }

    let entries = match read_ledger(ledger_path, true) {
        Ok(entries) => entries,
        Err(err) => {
            eprintln!("Failed to open ledger at {}: {}", ledger_path, err);
            exit(1);
        }
    };

    let head = match matches.value_of("head") {
        Some(head) => head.parse().expect("please pass a number for --head"),
        None => <usize>::max_value(),
    };

    match matches.subcommand() {
        ("print", _) => {
            let entries = match read_ledger(ledger_path, true) {
                Ok(entries) => entries,
                Err(err) => {
                    eprintln!("Failed to open ledger at {}: {}", ledger_path, err);
                    exit(1);
                }
            };
            for (i, entry) in entries.enumerate() {
                if i >= head {
                    break;
                }
                let entry = entry.unwrap();
                println!("{:?}", entry);
            }
        }
        ("json", _) => {
            stdout().write_all(b"{\"ledger\":[\n").expect("open array");
            for (i, entry) in entries.enumerate() {
                if i >= head {
                    break;
                }
                let entry = entry.unwrap();
                serde_json::to_writer(stdout(), &entry).expect("serialize");
                stdout().write_all(b",\n").expect("newline");
            }
            stdout().write_all(b"\n]}\n").expect("close array");
        }
        ("verify", _) => {
            const NUM_GENESIS_ENTRIES: usize = 3;
            if head < NUM_GENESIS_ENTRIES {
                eprintln!(
                    "verify requires at least {} entries to run",
                    NUM_GENESIS_ENTRIES
                );
                exit(1);
            }
            let bank = Bank::new_with_builtin_programs();
            {
                let genesis = match read_ledger(ledger_path, true) {
                    Ok(entries) => entries,
                    Err(err) => {
                        eprintln!("Failed to open ledger at {}: {}", ledger_path, err);
                        exit(1);
                    }
                };

                let genesis = genesis.take(NUM_GENESIS_ENTRIES).map(|e| e.unwrap());
                if let Err(e) = bank.process_ledger(genesis) {
                    eprintln!("verify failed at genesis err: {:?}", e);
                    if !matches.is_present("continue") {
                        exit(1);
                    }
                }
            }
            let entries = entries.map(|e| e.unwrap());

            let head = head - NUM_GENESIS_ENTRIES;

            let mut last_id = bank.last_id();

            for (i, entry) in entries.skip(NUM_GENESIS_ENTRIES).enumerate() {
                if i >= head {
                    break;
                }

                if !entry.verify(&last_id) {
                    eprintln!("entry.verify() failed at entry[{}]", i + 2);
                    if !matches.is_present("continue") {
                        exit(1);
                    }
                }
                last_id = entry.id;

                if let Err(e) = bank.process_entry(&entry) {
                    eprintln!("verify failed at entry[{}], err: {:?}", i + 2, e);
                    if !matches.is_present("continue") {
                        exit(1);
                    }
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
