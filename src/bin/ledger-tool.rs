#[macro_use]
extern crate clap;
extern crate serde_json;
extern crate solana;

use clap::{App, Arg, SubCommand};
use solana::bank::Bank;
use solana::ledger::read_ledger;
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
        .subcommand(SubCommand::with_name("print").about("Print the ledger"))
        .subcommand(SubCommand::with_name("verify").about("Verify the ledger"))
        .get_matches();

    let ledger_path = matches.value_of("ledger").unwrap();
    let entries = match read_ledger(ledger_path) {
        Ok(entries) => entries,
        Err(err) => {
            println!("Failed to open ledger at {}: {}", ledger_path, err);
            exit(1);
        }
    };

    match matches.subcommand() {
        ("print", _) => {
            stdout().write_all(b"{\"ledger\":[\n").expect("open array");
            for entry in entries {
                let entry = entry.unwrap();
                serde_json::to_writer(stdout(), &entry).expect("serialize");
                stdout().write_all(b",\n").expect("newline");
            }
            stdout().write_all(b"\n]}\n").expect("close array");
        }
        ("verify", _) => {
            let entries = entries.map(|entry| entry.unwrap());
            let bank = Bank::default();
            let entry_height = bank.process_ledger(entries).expect("process_ledger").0;
            println!("Ledger is valid.  Height: {}", entry_height);
        }
        ("", _) => {
            println!("{}", matches.usage());
        }
        _ => unreachable!(),
    };
}
