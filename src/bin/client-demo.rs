extern crate futures;
extern crate getopts;
extern crate isatty;
extern crate rayon;
extern crate serde_json;
extern crate solana;
extern crate untrusted;

use futures::Future;
use getopts::Options;
use isatty::stdin_isatty;
use rayon::prelude::*;
use solana::accountant_stub::AccountantStub;
use solana::mint::MintDemo;
use solana::signature::{KeyPair, KeyPairUtil};
use solana::transaction::Transaction;
use std::env;
use std::io::{stdin, Read};
use std::net::{SocketAddr, UdpSocket};
use std::process::exit;
use std::thread::sleep;
use std::time::Duration;
use std::time::Instant;
use untrusted::Input;

fn print_usage(program: &str, opts: Options) {
    let mut brief = format!("Usage: cat <mint.json> | {} [options]\n\n", program);
    brief += "  Solana client demo creates a number of transactions and\n";
    brief += "  sends them to a target node.";
    brief += "  Takes json formatted mint file to stdin.";

    print!("{}", opts.usage(&brief));
}

fn main() {
    let mut threads = 4usize;
    let mut addr: String = "127.0.0.1:8000".to_string();
    let mut client_addr: String = "127.0.0.1:8001".to_string();

    let mut opts = Options::new();
    opts.optopt("s", "", "server address", "host:port");
    opts.optopt("c", "", "client address", "host:port");
    opts.optopt("t", "", "number of threads", &format!("{}", threads));
    opts.optflag("h", "help", "print help");
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
    if matches.opt_present("s") {
        addr = matches.opt_str("s").unwrap();
    }
    if matches.opt_present("c") {
        client_addr = matches.opt_str("c").unwrap();
    }
    if matches.opt_present("t") {
        threads = matches.opt_str("t").unwrap().parse().expect("integer");
    }

    if stdin_isatty() {
        eprintln!("nothing found on stdin, expected a json file");
        exit(1);
    }

    let mut buffer = String::new();
    let num_bytes = stdin().read_to_string(&mut buffer).unwrap();
    if num_bytes == 0 {
        eprintln!("empty file on stdin, expected a json file");
        exit(1);
    }

    println!("Parsing stdin...");
    let demo: MintDemo = serde_json::from_str(&buffer).unwrap_or_else(|e| {
        eprintln!("failed to parse json: {}", e);
        exit(1);
    });

    println!("Binding to {}", client_addr);
    let socket = UdpSocket::bind(&client_addr).unwrap();
    socket.set_read_timeout(Some(Duration::new(5, 0))).unwrap();
    let mut acc = AccountantStub::new(addr.parse().unwrap(), socket);

    println!("Get last ID...");
    let last_id = acc.get_last_id().wait().unwrap();
    println!("Got last ID {:?}", last_id);

    println!("Creating keypairs...");
    let txs = demo.users.len() / 2;
    let keypairs: Vec<_> = demo.users
        .into_par_iter()
        .map(|(pkcs8, _)| KeyPair::from_pkcs8(Input::from(&pkcs8)).unwrap())
        .collect();
    let keypair_pairs: Vec<_> = keypairs.chunks(2).collect();

    println!("Signing transactions...");
    let now = Instant::now();
    let transactions: Vec<_> = keypair_pairs
        .into_par_iter()
        .map(|chunk| Transaction::new(&chunk[0], chunk[1].pubkey(), 1, last_id))
        .collect();
    let mut duration = now.elapsed();
    let ns = duration.as_secs() * 1_000_000_000 + u64::from(duration.subsec_nanos());
    let bsps = txs as f64 / ns as f64;
    let nsps = ns as f64 / txs as f64;
    println!(
        "Done. {} thousand signatures per second, {}us per signature",
        bsps * 1_000_000_f64,
        nsps / 1_000_f64
    );

    let initial_tx_count = acc.transaction_count();
    println!("initial count {}", initial_tx_count);

    println!("Transfering {} transactions in {} batches", txs, threads);
    let now = Instant::now();
    let sz = transactions.len() / threads;
    let chunks: Vec<_> = transactions.chunks(sz).collect();
    chunks.into_par_iter().for_each(|trs| {
        println!("Transferring 1 unit {} times... to", trs.len());
        let mut client_addr: SocketAddr = client_addr.parse().unwrap();
        client_addr.set_port(0);
        let socket = UdpSocket::bind(client_addr).unwrap();
        let acc = AccountantStub::new(addr.parse().unwrap(), socket);
        for tr in trs {
            acc.transfer_signed(tr.clone()).unwrap();
        }
    });

    println!("Waiting for transactions to complete...",);
    let mut tx_count;
    for _ in 0..10 {
        tx_count = acc.transaction_count();
        duration = now.elapsed();
        let txs = tx_count - initial_tx_count;
        println!("Transactions processed {}", txs);
        let ns = duration.as_secs() * 1_000_000_000 + u64::from(duration.subsec_nanos());
        let tps = (txs * 1_000_000_000) as f64 / ns as f64;
        println!("{} tps", tps);
        sleep(Duration::new(1, 0));
    }
}
