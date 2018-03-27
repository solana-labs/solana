extern crate solana;

use solana::entry::Entry;
use solana::event::Event;
use solana::hash::Hash;
use solana::historian::Historian;
use solana::ledger::verify_slice;
use solana::recorder::Signal;
use solana::signature::{KeyPair, KeyPairUtil};
use solana::transaction::Transaction;
use std::sync::mpsc::SendError;
use std::thread::sleep;
use std::time::Duration;

fn create_ledger(hist: &Historian, seed: &Hash) -> Result<(), SendError<Signal>> {
    sleep(Duration::from_millis(15));
    let keypair = KeyPair::new();
    let tr = Transaction::new(&keypair, keypair.pubkey(), 42, *seed);
    let signal0 = Signal::Event(Event::Transaction(tr));
    hist.sender.send(signal0)?;
    sleep(Duration::from_millis(10));
    Ok(())
}

fn main() {
    let seed = Hash::default();
    let hist = Historian::new(&seed, Some(10));
    create_ledger(&hist, &seed).expect("send error");
    drop(hist.sender);
    let entries: Vec<Entry> = hist.receiver.iter().collect();
    for entry in &entries {
        println!("{:?}", entry);
    }
    // Proof-of-History: Verify the historian learned about the events
    // in the same order they appear in the vector.
    assert!(verify_slice(&entries, &seed));
}
