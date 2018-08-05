extern crate clap;
extern crate serde_json;
extern crate solana;

use clap::{App, Arg};
use solana::ledger::read_ledger;
use std::io::{stdout, Write};

fn main() {
    let matches = App::new("ledger-view")
        .arg(
            Arg::with_name("ledger")
                .short("l")
                .long("ledger")
                .value_name("DIR")
                .takes_value(true)
                .required(true)
                .help("use DIR for ledger location"),
        )
        .get_matches();

    let ledger_path = matches.value_of("ledger").unwrap();

    let entries = read_ledger(ledger_path).expect("opening ledger");

    for entry in entries {
        let entry = entry.unwrap();
        serde_json::to_writer(stdout(), &entry).expect("serialize");
        stdout().write_all(b"\n").expect("newline");
    }
}
