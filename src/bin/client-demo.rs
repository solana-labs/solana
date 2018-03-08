extern crate serde_json;
extern crate silk;

use silk::accountant_stub::AccountantStub;
use silk::signature::{KeyPair, KeyPairUtil};
use silk::transaction::Transaction;
use silk::mint::Mint;
use std::time::Instant;
use std::net::UdpSocket;
use std::io::stdin;

fn main() {
    let addr = "127.0.0.1:8000";
    let send_addr = "127.0.0.1:8001";

    let mint: Mint = serde_json::from_reader(stdin()).unwrap();
    let mint_keypair = mint.keypair();
    let mint_pubkey = mint.pubkey();

    let socket = UdpSocket::bind(send_addr).unwrap();
    let mut acc = AccountantStub::new(addr, socket);
    let last_id = acc.get_last_id().unwrap();

    let txs = acc.get_balance(&mint_pubkey).unwrap().unwrap();
    println!("Mint's Initial Balance {}", txs);

    println!("Signing transactions...");
    let now = Instant::now();
    let transactions: Vec<_> = (0..txs)
        .map(|_| {
            let rando_pubkey = KeyPair::new().pubkey();
            Transaction::new(&mint_keypair, rando_pubkey, 1, last_id)
        })
        .collect();
    let duration = now.elapsed();
    let ns = duration.as_secs() * 1_000_000_000 + duration.subsec_nanos() as u64;
    let bsps = txs as f64 / ns as f64;
    let nsps = ns as f64 / txs as f64;
    println!(
        "Done. {} thousand signatures per second, {}us per signature",
        bsps * 1_000_000_f64,
        nsps / 1_000_f64
    );

    println!("Verify signatures...");
    let now = Instant::now();
    for tr in &transactions {
        assert!(tr.verify());
    }
    let duration = now.elapsed();
    let ns = duration.as_secs() * 1_000_000_000 + duration.subsec_nanos() as u64;
    let bsvps = txs as f64 / ns as f64;
    let nspsv = ns as f64 / txs as f64;
    println!(
        "Done. {} thousand signature verifications per second, {}us per signature verification",
        bsvps * 1_000_000_f64,
        nspsv / 1_000_f64
    );

    println!("Transferring 1 unit {} times...", txs);
    let now = Instant::now();
    let mut sig = Default::default();
    for tr in transactions {
        sig = tr.sig;
        acc.transfer_signed(tr).unwrap();
    }
    println!("Waiting for last transaction to be confirmed...",);
    acc.wait_on_signature(&sig).unwrap();

    let duration = now.elapsed();
    let ns = duration.as_secs() * 1_000_000_000 + duration.subsec_nanos() as u64;
    let tps = (txs * 1_000_000_000) as f64 / ns as f64;
    println!("Done. {} tps!", tps);
    let val = acc.get_balance(&mint_pubkey).unwrap().unwrap();
    println!("Mint's Final Balance {}", val);
    assert_eq!(val, 0);
}
