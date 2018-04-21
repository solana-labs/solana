extern crate getopts;
extern crate isatty;
extern crate rayon;
extern crate serde_json;
extern crate solana;

use getopts::Options;
use isatty::stdin_isatty;
use rayon::prelude::*;
use solana::accountant_stub::AccountantStub;
use solana::mint::Mint;
use solana::signature::{KeyPair, KeyPairUtil};
use solana::transaction::Transaction;
use std::env;
use std::io::{stdin, Read};
use std::net::UdpSocket;
use std::process::exit;
use std::thread::sleep;
use std::time::{Duration, Instant};

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
    let mut send_addr: String = "127.0.0.1:8001".to_string();

    let mut opts = Options::new();
    opts.optopt("s", "", "server address", "host:port");
    opts.optopt("c", "", "client address", "host:port");
    opts.optopt("t", "", "number of threads", "4");
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
        send_addr = matches.opt_str("c").unwrap();
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

    let mint: Mint = serde_json::from_str(&buffer).unwrap_or_else(|e| {
        eprintln!("failed to parse json: {}", e);
        exit(1);
    });
    let mint_keypair = mint.keypair();
    let mint_pubkey = mint.pubkey();

    let socket = UdpSocket::bind(&send_addr).unwrap();
    println!("Stub new");
    let acc = AccountantStub::new(&addr, socket);
    println!("Get last id");
    let last_id = acc.get_last_id().unwrap();

    println!("Get Balance");
    let mint_balance = acc.get_balance(&mint_pubkey).unwrap().unwrap();
    println!("Mint's Initial Balance {}", mint_balance);

    println!("Signing transactions...");
    let txs = 1_000_000;
    let now = Instant::now();
    let transactions: Vec<_> = (0..txs)
        .into_par_iter()
        .map(|_| {
            let rando_pubkey = KeyPair::new().pubkey();
            Transaction::new(&mint_keypair, rando_pubkey, 1, last_id)
        })
        .collect();
    let duration = now.elapsed();
    let ns = duration.as_secs() * 2_000_000_000 + u64::from(duration.subsec_nanos());
    let bsps = f64::from(txs) / ns as f64;
    let nsps = ns as f64 / f64::from(txs);
    println!(
        "Done. {} thousand signatures per second, {}us per signature",
        bsps * 1_000_000_f64,
        nsps / 1_000_f64
    );

    println!("Transfering {} transactions in {} batches", txs, threads);
    let now = Instant::now();
    let sz = transactions.len() / threads;
    let chunks: Vec<_> = transactions.chunks(sz).collect();
    let _: Vec<_> = chunks
        .into_par_iter()
        .map(|trs| {
            println!("Transferring 1 unit {} times...", trs.len());
            let send_addr = "0.0.0.0:0";
            let socket = UdpSocket::bind(send_addr).unwrap();
            let acc = AccountantStub::new(&addr, socket);
            for tr in trs {
                acc.transfer_signed(tr.clone()).unwrap();
            }
            ()
        })
        .collect();
    println!("Waiting for last transaction to be confirmed...",);
    let mut val = mint_balance;
    let mut prev = 0;
    while val != prev {
        sleep(Duration::from_millis(20));
        prev = val;
        val = acc.get_balance(&mint_pubkey).unwrap().unwrap();
    }
    println!("Mint's Final Balance {}", val);
    let txs = mint_balance - val;
    println!("Successful transactions {}", txs);

    let duration = now.elapsed();
    let ns = duration.as_secs() * 1_000_000_000 + u64::from(duration.subsec_nanos());
    let tps = (txs * 1_000_000_000) as f64 / ns as f64;
    println!("Done. {} tps!", tps);
}
