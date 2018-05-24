extern crate env_logger;
extern crate getopts;
extern crate isatty;
extern crate pnet;
extern crate serde_json;
extern crate solana;

use getopts::Options;
use isatty::stdin_isatty;
use pnet::datalink;
use solana::bank::Bank;
use solana::crdt::ReplicatedData;
use solana::entry::Entry;
use solana::event::Event;
use solana::server::Server;
use solana::signature::{KeyPair, KeyPairUtil};
use solana::transaction::Instruction;
use std::env;
use std::fs::File;
use std::io::{stdin, stdout, Read};
use std::net::{IpAddr, SocketAddr, UdpSocket};
use std::process::exit;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

fn print_usage(program: &str, opts: Options) {
    let mut brief = format!("Usage: cat <transaction.log> | {} [options]\n\n", program);
    brief += "  Run a Solana node to handle transactions and\n";
    brief += "  write a new transaction log to stdout.\n";
    brief += "  Takes existing transaction log from stdin.";

    print!("{}", opts.usage(&brief));
}

fn main() {
    env_logger::init().unwrap();
    let mut opts = Options::new();
    opts.optopt("b", "", "bind", "bind to port or address");
    opts.optflag("d", "dyn", "detect network address dynamically");
    opts.optopt("s", "", "save", "save my identity to path.json");
    opts.optflag("h", "help", "print help");
    opts.optopt(
        "v",
        "",
        "validator",
        "run as replicate with path to leader.json",
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
    let bind_addr: SocketAddr = {
        let mut bind_addr = parse_port_or_addr(matches.opt_str("b"));
        if matches.opt_present("d") {
            let ip = get_ip_addr().unwrap();
            bind_addr.set_ip(ip);
        }
        bind_addr
    };
    if stdin_isatty() {
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
    let Event::Transaction(ref tr) = entry1.events[0];
    let deposit = if let Instruction::NewContract(contract) = &tr.instruction {
        contract.plan.final_payment()
    } else {
        None
    };

    eprintln!("creating bank...");

    let bank = Bank::new_from_deposit(&deposit.unwrap());
    bank.register_entry_id(&entry0.id);
    bank.register_entry_id(&entry1.id);

    eprintln!("processing entries...");

    let mut last_id = entry1.id;
    for entry in entries {
        last_id = entry.id;
        let results = bank.process_verified_events(entry.events);
        for result in results {
            if let Err(e) = result {
                eprintln!("failed to process event {:?}", e);
                exit(1);
            }
        }
        bank.register_entry_id(&last_id);
    }

    eprintln!("creating networking stack...");

    let exit = Arc::new(AtomicBool::new(false));
    // we need all the receiving sockets to be bound within the expected
    // port range that we open on aws
    let mut repl_data = make_repl_data(&bind_addr);
    let threads = if matches.opt_present("v") {
        eprintln!("starting validator... {}", repl_data.requests_addr);
        let path = matches.opt_str("v").unwrap();
        let file = File::open(path).expect("file");
        let leader = serde_json::from_reader(file).expect("parse");
        let s = Server::new_validator(
            bank,
            repl_data.clone(),
            UdpSocket::bind(repl_data.requests_addr).unwrap(),
            UdpSocket::bind("0.0.0.0:0").unwrap(),
            UdpSocket::bind(repl_data.replicate_addr).unwrap(),
            UdpSocket::bind(repl_data.gossip_addr).unwrap(),
            leader,
            exit.clone(),
        );
        s.thread_hdls
    } else {
        eprintln!("starting leader... {}", repl_data.requests_addr);
        repl_data.current_leader_id = repl_data.id.clone();
        let server = Server::new_leader(
            bank,
            last_id,
            Some(Duration::from_millis(1000)),
            repl_data.clone(),
            UdpSocket::bind(repl_data.requests_addr).unwrap(),
            UdpSocket::bind(repl_data.events_addr).unwrap(),
            UdpSocket::bind("0.0.0.0:0").unwrap(),
            UdpSocket::bind("0.0.0.0:0").unwrap(),
            UdpSocket::bind(repl_data.gossip_addr).unwrap(),
            exit.clone(),
            stdout(),
        );
        server.thread_hdls
    };
    if matches.opt_present("s") {
        let path = matches.opt_str("s").unwrap();
        let file = File::create(path).expect("file");
        serde_json::to_writer(file, &repl_data).expect("serialize");
    }
    eprintln!("Ready. Listening on {}", bind_addr);

    for t in threads {
        t.join().expect("join");
    }
}

fn next_port(server_addr: &SocketAddr, nxt: u16) -> SocketAddr {
    let mut gossip_addr = server_addr.clone();
    gossip_addr.set_port(server_addr.port() + nxt);
    gossip_addr
}

fn make_repl_data(bind_addr: &SocketAddr) -> ReplicatedData {
    let events_addr = bind_addr.clone();
    let gossip_addr = next_port(&bind_addr, 1);
    let replicate_addr = next_port(&bind_addr, 2);
    let requests_addr = next_port(&bind_addr, 3);
    let pubkey = KeyPair::new().pubkey();
    ReplicatedData::new(
        pubkey,
        gossip_addr,
        replicate_addr,
        requests_addr,
        events_addr,
    )
}

fn parse_port_or_addr(optstr: Option<String>) -> SocketAddr {
    let daddr: SocketAddr = "0.0.0.0:8000".parse().expect("default socket address");
    if let Some(addrstr) = optstr {
        if let Ok(port) = addrstr.parse() {
            let mut addr = daddr.clone();
            addr.set_port(port);
            addr
        } else if let Ok(addr) = addrstr.parse() {
            addr
        } else {
            daddr
        }
    } else {
        daddr
    }
}

fn get_ip_addr() -> Option<IpAddr> {
    for iface in datalink::interfaces() {
        for p in iface.ips {
            if !p.ip().is_loopback() && !p.ip().is_multicast() {
                return Some(p.ip());
            }
        }
    }
    None
}

#[test]
fn test_parse_port_or_addr() {
    let p1 = parse_port_or_addr(Some("9000".to_string()));
    assert_eq!(p1.port(), 9000);
    let p2 = parse_port_or_addr(Some("127.0.0.1:7000".to_string()));
    assert_eq!(p2.port(), 7000);
    let p3 = parse_port_or_addr(None);
    assert_eq!(p3.port(), 8000);
}
