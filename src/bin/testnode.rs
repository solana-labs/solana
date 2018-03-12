extern crate serde_json;
extern crate silk;

use silk::accountant_skel::AccountantSkel;
use silk::accountant::Accountant;
use std::io::{self, BufRead};
use std::sync::{Arc, Mutex};

fn main() {
    let addr = "127.0.0.1:8000";
    let stdin = io::stdin();
    let entries = stdin
        .lock()
        .lines()
        .map(|line| serde_json::from_str(&line.unwrap()).unwrap());
    let acc = Accountant::new_from_entries(entries, Some(1000));
    let exit = Arc::new(Mutex::new(false));
    let skel = Arc::new(Mutex::new(AccountantSkel::new(acc)));
    eprintln!("Listening on {}", addr);
    let _threads = AccountantSkel::serve(skel, addr, exit.clone()).unwrap();
}
