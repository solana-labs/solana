extern crate rayon;
extern crate serde_json;
extern crate solana;

use rayon::prelude::*;
use solana::accountant_stub::AccountantStub;
use solana::mint::Mint;
use solana::signature::{KeyPair, KeyPairUtil};
use solana::transaction::Transaction;
use std::io::stdin;
use std::net::UdpSocket;
use std::thread::sleep;
use std::time::{Duration, Instant};

fn main() {
    let addr = "127.0.0.1:8000";
    let send_addr = "127.0.0.1:8001";

    let mint: Mint = serde_json::from_reader(stdin()).unwrap();
    let mint_keypair = mint.keypair();
    let mint_pubkey = mint.pubkey();

    let socket = UdpSocket::bind(send_addr).unwrap();
    let acc = AccountantStub::new(addr, socket);
    let last_id = acc.get_last_id().unwrap();

    let mint_balance = acc.get_balance(&mint_pubkey).unwrap().unwrap();
    println!("Mint's Initial Balance {}", mint_balance);

    println!("Signing transactions...");
    let txs = 100_000;
    let now = Instant::now();
    let transactions: Vec<_> = (0..txs)
        .into_par_iter()
        .map(|_| {
            let rando_pubkey = KeyPair::new().pubkey();
            Transaction::new(&mint_keypair, rando_pubkey, 1, last_id)
        })
        .collect();
    let duration = now.elapsed();
    let ns = duration.as_secs() * 1_000_000_000 + u64::from(duration.subsec_nanos());
    let bsps = f64::from(txs) / ns as f64;
    let nsps = ns as f64 / f64::from(txs);
    println!(
        "Done. {} thousand signatures per second, {}us per signature",
        bsps * 1_000_000_f64,
        nsps / 1_000_f64
    );

    println!("Transferring 1 unit {} times...", txs);
    let now = Instant::now();
    let mut _sig = Default::default();
    for tr in transactions {
        _sig = tr.sig;
        acc.transfer_signed(tr).unwrap();
    }
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
