extern crate serde_json;
extern crate solana;

use solana::accountant::Accountant;
use solana::accountant_skel::AccountantSkel;
use solana::entry::Entry;
use solana::event::Event;
use solana::historian::Historian;
use std::io::{self, stdout, BufRead};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

fn main() {
    let addr = "127.0.0.1:8000";
    let stdin = io::stdin();
    let mut entries = stdin
        .lock()
        .lines()
        .map(|line| serde_json::from_str(&line.unwrap()).unwrap());

    // The first item in the ledger is required to be an entry with zero num_hashes,
    // which implies its id can be used as the ledger's seed.
    entries.next().unwrap();

    // The second item in the ledger is a special transaction where the to and from
    // fields are the same. That entry should be treated as a deposit, not a
    // transfer to oneself.
    let entry1: Entry = entries.next().unwrap();
    let deposit = if let Event::Transaction(ref tr) = entry1.events[0] {
        tr.plan.final_payment()
    } else {
        None
    };

    let mut acc = Accountant::new_from_deposit(&deposit.unwrap());

    let mut last_id = entry1.id;
    for entry in entries {
        last_id = entry.id;
        for event in entry.events {
            acc.process_verified_event(&event).unwrap();
        }
    }

    let historian = Historian::new(&last_id, Some(1000));
    let exit = Arc::new(AtomicBool::new(false));
    let skel = Arc::new(Mutex::new(AccountantSkel::new(
        acc,
        last_id,
        stdout(),
        historian,
    )));
    eprintln!("Listening on {}", addr);
    let threads = AccountantSkel::serve(skel, addr, exit.clone()).unwrap();
    for t in threads {
        t.join().expect("join");
    }
}
