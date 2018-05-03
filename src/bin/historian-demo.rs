extern crate solana;

use solana::entry::Entry;
use solana::event::Event;
use solana::hash::Hash;
use solana::historian::Historian;
use solana::ledger::Block;
use solana::recorder::Signal;
use solana::signature::{KeyPair, KeyPairUtil};
use solana::transaction::Transaction;
use std::sync::mpsc::{sync_channel, SendError, SyncSender};
use std::thread::sleep;
use std::time::Duration;

fn create_ledger(input: &SyncSender<Signal>, seed: &Hash) -> Result<(), SendError<Signal>> {
    sleep(Duration::from_millis(15));
    let keypair = KeyPair::new();
    let tr = Transaction::new(&keypair, keypair.pubkey(), 42, *seed);
    let signal0 = Signal::Event(Event::Transaction(tr));
    input.send(signal0)?;
    sleep(Duration::from_millis(10));
    Ok(())
}

fn main() {
    let (input, event_receiver) = sync_channel(10);
    let seed = Hash::default();
    let hist = Historian::new(event_receiver, &seed, Some(10));
    create_ledger(&input, &seed).expect("send error");
    drop(input);
    let entries: Vec<Entry> = hist.output.iter().collect();
    for entry in &entries {
        println!("{:?}", entry);
    }
    // Proof-of-History: Verify the historian learned about the events
    // in the same order they appear in the vector.
    assert!(entries[..].verify(&seed));
}
