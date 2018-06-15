extern crate atty;
extern crate env_logger;
extern crate getopts;
extern crate serde_json;
extern crate solana;
#[macro_use]
extern crate log;

use atty::{is, Stream};
use getopts::Options;
use solana::bank::Bank;
use solana::crdt::ReplicatedData;
use solana::entry::Entry;
use solana::payment_plan::PaymentPlan;
use solana::server::Server;
use solana::transaction::Instruction;
use std::env;
use std::fs::File;
use std::io::{stdin, stdout, Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};
use std::process::exit;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
//use std::time::Duration;

fn print_usage(program: &str, opts: Options) {
    let mut brief = format!("Usage: cat <transaction.log> | {} [options]\n\n", program);
    brief += "  Run a Solana node to handle transactions and\n";
    brief += "  write a new transaction log to stdout.\n";
    brief += "  Takes existing transaction log from stdin.";

    print!("{}", opts.usage(&brief));
}

fn main() {
    env_logger::init();
    let mut opts = Options::new();
    opts.optflag("h", "help", "print help");
    opts.optopt("l", "", "run with the identity found in FILE", "FILE");
    opts.optopt("v", "", "validate; find leader's identity in FILE", "FILE");
    opts.optopt(
        "o",
        "",
        "output log to FILE, defaults to stdout (ignored by validators)",
        "FILE",
    );

    let args: Vec<String> = env::args().collect();
    let matches = match opts.parse(&args[1..]) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("{}", e);
            exit(1);
        }
    };
    if matches.opt_present("h") {
        let program = args[0].clone();
        print_usage(&program, opts);
        return;
    }
    if is(Stream::Stdin) {
        eprintln!("nothing found on stdin, expected a log file");
        exit(1);
    }

    let mut buffer = String::new();
    let num_bytes = stdin().read_to_string(&mut buffer).unwrap();
    if num_bytes == 0 {
        eprintln!("empty file on stdin, expected a log file");
        exit(1);
    }

    eprintln!("Initializing...");
    let mut entries = buffer.lines().map(|line| {
        serde_json::from_str(&line).unwrap_or_else(|e| {
            eprintln!("failed to parse json: {}", e);
            exit(1);
        })
    });

    eprintln!("done parsing...");

    // The first item in the ledger is required to be an entry with zero num_hashes,
    // which implies its id can be used as the ledger's seed.
    let entry0 = entries.next().unwrap();

    // The second item in the ledger is a special transaction where the to and from
    // fields are the same. That entry should be treated as a deposit, not a
    // transfer to oneself.
    let entry1: Entry = entries.next().unwrap();
    let tx = &entry1.transactions[0];
    let deposit = if let Instruction::NewContract(contract) = &tx.instruction {
        contract.plan.final_payment()
    } else {
        None
    };

    eprintln!("creating bank...");

    let bank = Bank::new_from_deposit(&deposit.unwrap());
    bank.register_entry_id(&entry0.id);
    bank.register_entry_id(&entry1.id);

    eprintln!("processing entries...");
    bank.process_entries(entries).expect("process_entries");

    eprintln!("creating networking stack...");

    let exit = Arc::new(AtomicBool::new(false));
    let bind_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), 8000);
    let mut repl_data = ReplicatedData::new_leader(&bind_addr);
    if matches.opt_present("l") {
        let path = matches.opt_str("l").unwrap();
        if let Ok(file) = File::open(path.clone()) {
            if let Ok(data) = serde_json::from_reader(file) {
                repl_data = data;
            } else {
                warn!("failed to parse {}, generating new identity", path);
            }
        } else {
            warn!("failed to read {}, generating new identity", path);
        }
    }

    let threads = if matches.opt_present("v") {
        let path = matches.opt_str("v").unwrap();
        eprintln!(
            "starting validator... {} using {}",
            repl_data.requests_addr, path
        );
        let file = File::open(path.clone()).expect(&format!("file not found: {}", path));
        let leader = serde_json::from_reader(file).expect("parse");
        let s = Server::new_validator(
            bank,
            repl_data.clone(),
            UdpSocket::bind(repl_data.requests_addr).unwrap(),
            UdpSocket::bind("0.0.0.0:0").unwrap(),
            UdpSocket::bind(repl_data.replicate_addr).unwrap(),
            UdpSocket::bind(repl_data.gossip_addr).unwrap(),
            UdpSocket::bind(repl_data.repair_addr).unwrap(),
            leader,
            exit.clone(),
        );
        s.thread_hdls
    } else {
        eprintln!("starting leader... {}", repl_data.requests_addr);
        repl_data.current_leader_id = repl_data.id.clone();

        let outfile: Box<Write + Send + 'static> = if matches.opt_present("o") {
            let path = matches.opt_str("o").unwrap();
            Box::new(
                File::create(&path).expect(&format!("unable to open output file \"{}\"", path)),
            )
        } else {
            Box::new(stdout())
        };

        let server = Server::new_leader(
            bank,
            //Some(Duration::from_millis(1000)),
            None,
            repl_data.clone(),
            UdpSocket::bind(repl_data.requests_addr).unwrap(),
            UdpSocket::bind(repl_data.transactions_addr).unwrap(),
            UdpSocket::bind("0.0.0.0:0").unwrap(),
            UdpSocket::bind("0.0.0.0:0").unwrap(),
            UdpSocket::bind(repl_data.gossip_addr).unwrap(),
            exit.clone(),
            outfile,
        );
        server.thread_hdls
    };
    eprintln!("Ready. Listening on {}", repl_data.transactions_addr);

    for t in threads {
        t.join().expect("join");
    }
}
