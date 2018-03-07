extern crate silk;

use silk::historian::Historian;
use silk::hash::Hash;
use silk::entry::Entry;
use silk::log::verify_slice;
use silk::signature::{generate_keypair, get_pubkey};
use silk::transaction::Transaction;
use silk::event::Event;
use std::thread::sleep;
use std::time::Duration;
use std::sync::mpsc::SendError;

fn create_log(hist: &Historian<Hash>, seed: &Hash) -> Result<(), SendError<Event<Hash>>> {
    sleep(Duration::from_millis(15));
    let asset = Hash::default();
    let keypair = generate_keypair();
    let tr = Transaction::new(&keypair, get_pubkey(&keypair), asset, *seed);
    let event0 = Event::Transaction(tr);
    hist.sender.send(event0)?;
    sleep(Duration::from_millis(10));
    Ok(())
}

fn main() {
    let seed = Hash::default();
    let hist = Historian::new(&seed, Some(10));
    create_log(&hist, &seed).expect("send error");
    drop(hist.sender);
    let entries: Vec<Entry<Hash>> = hist.receiver.iter().collect();
    for entry in &entries {
        println!("{:?}", entry);
    }
    // Proof-of-History: Verify the historian learned about the events
    // in the same order they appear in the vector.
    assert!(verify_slice(&entries, &seed));
}
