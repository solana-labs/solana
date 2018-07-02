extern crate atty;
extern crate env_logger;
extern crate getopts;
extern crate log;
extern crate serde_json;
extern crate solana;

use atty::{is, Stream};
use getopts::Options;
use solana::bank::Bank;
use solana::crdt::ReplicatedData;
use solana::entry_writer;
use solana::server::Server;
use std::env;
use std::fs::File;
use std::io::{stdin, stdout, BufRead, Write};
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
    opts.optopt(
        "t",
        "",
        "testnet; connect to the network at this gossip entry point",
        "HOST:PORT",
    );
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

    eprintln!("Initializing...");
    let stdin = stdin();
    let entries = stdin.lock().lines().map(|line| {
        entry_writer::read_entry(line.unwrap()).unwrap_or_else(|e| {
            eprintln!("failed to parse json: {}", e);
            exit(1);
        })
    });
    eprintln!("done parsing...");

    eprintln!("creating bank...");
    let bank = Bank::default();

    // entry_height is the network-wide agreed height of the ledger.
    //  initialize it from the input ledger
    eprintln!("processing ledger...");
    let entry_height = bank.process_ledger(entries).expect("process_ledger");
    eprintln!("processed {} ledger...", entry_height);

    eprintln!("creating networking stack...");

    let bind_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), 8000);
    let mut repl_data = ReplicatedData::new_leader(&bind_addr);
    if matches.opt_present("l") {
        let path = matches.opt_str("l").unwrap();
        if let Ok(file) = File::open(path.clone()) {
            if let Ok(data) = serde_json::from_reader(file) {
                repl_data = data;
            } else {
                eprintln!("failed to parse {}", path);
                exit(1);
            }
        } else {
            eprintln!("failed to read {}", path);
            exit(1);
        }
    }

    let mut local_gossip_addr = bind_addr.clone();
    local_gossip_addr.set_port(repl_data.gossip_addr.port());

    let mut local_replicate_addr = bind_addr.clone();
    local_replicate_addr.set_port(repl_data.replicate_addr.port());

    let mut local_requests_addr = bind_addr.clone();
    local_requests_addr.set_port(repl_data.requests_addr.port());

    let mut local_transactions_addr = bind_addr.clone();
    local_transactions_addr.set_port(repl_data.transactions_addr.port());

    let mut local_repair_addr = bind_addr.clone();
    local_repair_addr.set_port(repl_data.repair_addr.port());

    let exit = Arc::new(AtomicBool::new(false));
    let threads = if matches.opt_present("t") {
        let testnet_address_string = matches.opt_str("t").unwrap();
        eprintln!(
            "starting validator... {} (advertising {}) connecting to {}",
            local_requests_addr, repl_data.requests_addr, testnet_address_string
        );
        let testnet_addr = testnet_address_string.parse().unwrap();
        let newtwork_entry_point = ReplicatedData::new_entry_point(testnet_addr);
        let s = Server::new_validator(
            bank,
            entry_height,
            repl_data.clone(),
            UdpSocket::bind(local_requests_addr).unwrap(),
            UdpSocket::bind("0.0.0.0:0").unwrap(),
            UdpSocket::bind(local_replicate_addr).unwrap(),
            UdpSocket::bind(local_gossip_addr).unwrap(),
            UdpSocket::bind(local_repair_addr).unwrap(),
            newtwork_entry_point,
            exit.clone(),
        );
        s.thread_hdls
    } else {
        eprintln!(
            "starting leader... {} (advertising {})",
            local_requests_addr, repl_data.requests_addr
        );
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
            entry_height,
            //Some(Duration::from_millis(1000)),
            None,
            repl_data.clone(),
            UdpSocket::bind(local_requests_addr).unwrap(),
            UdpSocket::bind(local_transactions_addr).unwrap(),
            UdpSocket::bind("0.0.0.0:0").unwrap(),
            UdpSocket::bind("0.0.0.0:0").unwrap(),
            UdpSocket::bind(local_gossip_addr).unwrap(),
            exit.clone(),
            outfile,
        );
        server.thread_hdls
    };
    eprintln!(
        "Ready. Listening on {} (advertising {})",
        local_transactions_addr, repl_data.transactions_addr
    );

    for t in threads {
        t.join().expect("join");
    }
}
