//! A command-line executable for generating the chain's genesis block.

extern crate ring;
extern crate serde_json;
extern crate silk;

use silk::genesis::Genesis;
use silk::log::{hash, verify_slice_u64};
use silk::logger::Logger;
use std::sync::mpsc::sync_channel;
use std::io::stdin;

fn main() {
    let gen: Genesis = serde_json::from_reader(stdin()).unwrap();

    let (_sender, event_receiver) = sync_channel(100);
    let (entry_sender, receiver) = sync_channel(100);
    let mut logger = Logger::new(event_receiver, entry_sender, hash(&gen.pkcs8));
    for tx in gen.create_events() {
        logger.log_event(tx).unwrap();
    }
    drop(logger.sender);

    let entries = receiver.iter().collect::<Vec<_>>();
    verify_slice_u64(&entries, &entries[0].end_hash);
    println!("[");
    let len = entries.len();
    for (i, x) in entries.iter().enumerate() {
        let s = serde_json::to_string(&x).unwrap();

        let terminator = if i + 1 == len { "" } else { "," };
        println!("    {}{}", s, terminator);
    }
    println!("]");
}
