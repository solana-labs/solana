extern crate atty;
extern crate env_logger;
extern crate getopts;
extern crate log;
extern crate serde_json;
extern crate solana;

use atty::{is, Stream};
use getopts::Options;
use solana::crdt::{ReplicatedData, TestNode};
use solana::fullnode::start;
use std::env;
use std::fs::File;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
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

fn main() -> () {
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
    let mut node = TestNode::new_with_bind_addr(repl_data, bind_addr);
    let exit = Arc::new(AtomicBool::new(false));
    let threads = if matches.opt_present("t") {
        let testnet_address_string = matches.opt_str("t").unwrap();
        let testnet_addr = testnet_address_string.parse().unwrap();
        start(node, false, None, Some(testnet_addr), None, exit)
    } else {
        node.data.current_leader_id = node.data.id.clone();

        let outfile = matches.opt_str("o");
        start(node, true, None, None, outfile, exit)
    };
    for t in threads {
        t.join().expect("join");
    }
}
