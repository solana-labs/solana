extern crate futures;
extern crate getopts;
extern crate isatty;
extern crate rayon;
extern crate serde_json;
extern crate solana;

use futures::Future;
use getopts::Options;
use isatty::stdin_isatty;
use rayon::prelude::*;
use solana::mint::MintDemo;
use solana::signature::{GenKeys, KeyPairUtil};
use solana::thin_client::ThinClient;
use solana::transaction::Transaction;
use std::env;
use std::io::{stdin, Read};
use std::net::{SocketAddr, UdpSocket};
use std::process::exit;
use std::thread::sleep;
use std::time::Duration;
use std::time::Instant;

fn print_usage(program: &str, opts: Options) {
    let mut brief = format!("Usage: cat <mint.json> | {} [options]\n\n", program);
    brief += "  Solana client demo creates a number of transactions and\n";
    brief += "  sends them to a target node.";
    brief += "  Takes json formatted mint file to stdin.";

    print!("{}", opts.usage(&brief));
}

fn main() {
    let mut threads = 4usize;
    let mut server_addr: String = "127.0.0.1:8000".to_string();
    let mut requests_addr: String = "127.0.0.1:8010".to_string();

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
        server_addr = matches.opt_str("s").unwrap();
    }
    if matches.opt_present("c") {
        requests_addr = matches.opt_str("c").unwrap();
    }
    if matches.opt_present("t") {
        threads = matches.opt_str("t").unwrap().parse().expect("integer");
    }

    let mut transactions_addr: SocketAddr = requests_addr.parse().unwrap();
    let requests_port = transactions_addr.port();
    transactions_addr.set_port(requests_port + 1);

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

    println!("Binding to {}", requests_addr);
    let requests_socket = UdpSocket::bind(&requests_addr).unwrap();
    requests_socket
        .set_read_timeout(Some(Duration::new(5, 0)))
        .unwrap();
    let transactions_socket = UdpSocket::bind(&transactions_addr).unwrap();
    let requests_addr: SocketAddr = server_addr.parse().unwrap();
    let requests_port = requests_addr.port();
    let mut transactions_addr = requests_addr.clone();
    transactions_addr.set_port(requests_port + 3);
    let mut client = ThinClient::new(
        requests_addr,
        requests_socket,
        transactions_addr,
        transactions_socket,
    );

    println!("Get last ID...");
    let last_id = client.get_last_id().wait().unwrap();
    println!("Got last ID {:?}", last_id);

    let rnd = GenKeys::new(demo.mint.keypair().public_key_bytes());

    println!("Creating keypairs...");
    let txs = demo.num_accounts / 2;
    let keypairs = rnd.gen_n_keypairs(demo.num_accounts);
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

    let initial_tx_count = client.transaction_count();
    println!("initial count {}", initial_tx_count);

    println!("Transfering {} transactions in {} batches", txs, threads);
    let now = Instant::now();
    let sz = transactions.len() / threads;
    let chunks: Vec<_> = transactions.chunks(sz).collect();
    chunks.into_par_iter().for_each(|txs| {
        println!("Transferring 1 unit {} times... to", txs.len());
        let requests_addr: SocketAddr = server_addr.parse().unwrap();
        let mut requests_cb_addr = requests_addr.clone();
        requests_cb_addr.set_port(0);
        let requests_socket = UdpSocket::bind(requests_cb_addr).unwrap();
        requests_socket
            .set_read_timeout(Some(Duration::new(5, 0)))
            .unwrap();
        let mut transactions_addr: SocketAddr = requests_addr.clone();
        transactions_addr.set_port(0);
        let transactions_socket = UdpSocket::bind(&transactions_addr).unwrap();
        let client = ThinClient::new(
            requests_addr,
            requests_socket,
            transactions_addr,
            transactions_socket,
        );
        for tx in txs {
            client.transfer_signed(tx.clone()).unwrap();
        }
    });

    println!("Waiting for transactions to complete...",);
    let mut tx_count;
    for _ in 0..10 {
        tx_count = client.transaction_count();
        duration = now.elapsed();
        let txs = tx_count - initial_tx_count;
        println!("Transactions processed {}", txs);
        let ns = duration.as_secs() * 1_000_000_000 + u64::from(duration.subsec_nanos());
        let tps = (txs * 1_000_000_000) as f64 / ns as f64;
        println!("{} tps", tps);
        sleep(Duration::new(1, 0));
    }
}
