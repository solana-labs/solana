use clap::{crate_version, App, Arg, SubCommand};
use solana::bank::Bank;
use solana::db_ledger::DbLedger;
use solana::genesis_block::GenesisBlock;
use std::io::{stdout, Write};
use std::process::exit;

fn main() {
    solana_logger::setup();
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
            Arg::with_name("min-hashes")
                .short("h")
                .long("min-hashes")
                .value_name("NUM")
                .takes_value(true)
                .help("Skip entries with fewer than NUM hashes\n  (only applies to print and json commands)"),
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

    let genesis_block = GenesisBlock::load(ledger_path).unwrap_or_else(|err| {
        eprintln!(
            "Failed to open ledger genesis_block at {}: {}",
            ledger_path, err
        );
        exit(1);
    });

    let db_ledger = match DbLedger::open(ledger_path) {
        Ok(db_ledger) => db_ledger,
        Err(err) => {
            eprintln!("Failed to open ledger at {}: {}", ledger_path, err);
            exit(1);
        }
    };

    let entries = match db_ledger.read_ledger() {
        Ok(entries) => entries,
        Err(err) => {
            eprintln!("Failed to read ledger at {}: {}", ledger_path, err);
            exit(1);
        }
    };

    let head = match matches.value_of("head") {
        Some(head) => head.parse().expect("please pass a number for --head"),
        None => <usize>::max_value(),
    };

    let min_hashes = match matches.value_of("min-hashes") {
        Some(hashes) => hashes
            .parse()
            .expect("please pass a number for --min-hashes"),
        None => 0,
    } as u64;

    match matches.subcommand() {
        ("print", _) => {
            for (i, entry) in entries.enumerate() {
                if i >= head {
                    break;
                }

                if entry.num_hashes < min_hashes {
                    continue;
                }
                println!("{:?}", entry);
            }
        }
        ("json", _) => {
            stdout().write_all(b"{\"ledger\":[\n").expect("open array");
            for (i, entry) in entries.enumerate() {
                if i >= head {
                    break;
                }

                if entry.num_hashes < min_hashes {
                    continue;
                }
                serde_json::to_writer(stdout(), &entry).expect("serialize");
                stdout().write_all(b",\n").expect("newline");
            }
            stdout().write_all(b"\n]}\n").expect("close array");
        }
        ("verify", _) => {
            let bank = Bank::new(&genesis_block);
            let mut last_id = bank.active_fork().last_id();
            let mut num_entries = 0;
            for (i, entry) in entries.enumerate() {
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
                num_entries += 1;

                if let Err(e) = bank.active_fork().process_entries(&[entry]) {
                    eprintln!("verify failed at entry[{}], err: {:?}", i + 2, e);
                    if !matches.is_present("continue") {
                        exit(1);
                    }
                }
            }
            println!("{} entries.  last_id={:?}", num_entries, last_id);
        }
        ("", _) => {
            eprintln!("{}", matches.usage());
            exit(1);
        }
        _ => unreachable!(),
    };
}
