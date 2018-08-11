#[macro_use]
extern crate clap;
extern crate serde_json;
extern crate solana;

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
                .help("use DIR for ledger location"),
        )
        .arg(
            Arg::with_name("head")
                .short("n")
                .long("head")
                .value_name("NUM")
                .takes_value(true)
                .help("at most the first NUM entries in ledger\n  (only applies to verify, print, json commands)"),
        )
        .arg(
            Arg::with_name("precheck")
                .short("p")
                .long("precheck")
                .help("use ledger_verify() to check internal ledger consistency before proceeding"),
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
            let bank = Bank::default();
            if head != <usize>::max_value() {
                let entries = entries.map(|entry| entry.unwrap()).take(head);
                bank.process_ledger(entries).expect("process_ledger");
            } else {
                let entries = entries.map(|entry| entry.unwrap());
                bank.process_ledger(entries).expect("process_ledger");
            }
        }
        ("", _) => {
            eprintln!("{}", matches.usage());
            exit(1);
        }
        _ => unreachable!(),
    };
}
