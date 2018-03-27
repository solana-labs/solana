extern crate serde_json;
extern crate solana;

use solana::accountant::Accountant;
use solana::accountant_skel::AccountantSkel;
use std::io::{self, stdout, BufRead};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

fn main() {
    let addr = "127.0.0.1:8000";
    let stdin = io::stdin();
    let entries = stdin
        .lock()
        .lines()
        .map(|line| serde_json::from_str(&line.unwrap()).unwrap());
    let acc = Accountant::new_from_entries(entries, Some(1000));
    let exit = Arc::new(AtomicBool::new(false));
    let skel = Arc::new(Mutex::new(AccountantSkel::new(acc, stdout())));
    eprintln!("Listening on {}", addr);
    let threads = AccountantSkel::serve(skel, addr, exit.clone()).unwrap();
    for t in threads {
        t.join().expect("join");
    }
}
