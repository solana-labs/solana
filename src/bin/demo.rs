extern crate silk;

use silk::historian::Historian;
use silk::log::{verify_slice, Entry, Event, Sha256Hash};
use std::{thread, time};
use std::sync::mpsc::SendError;

fn create_log(hist: &Historian) -> Result<(), SendError<Event>> {
    hist.sender.send(Event::Tick)?;
    thread::sleep(time::Duration::new(0, 100_000));
    hist.sender.send(Event::UserDataKey(Sha256Hash::default()))?;
    thread::sleep(time::Duration::new(0, 100_000));
    hist.sender.send(Event::Tick)?;
    Ok(())
}

fn main() {
    let seed = Sha256Hash::default();
    let hist = Historian::new(&seed, None);
    create_log(&hist).expect("send error");
    drop(hist.sender);
    let entries: Vec<Entry> = hist.receiver.iter().collect();
    for entry in &entries {
        println!("{:?}", entry);
    }
    assert!(verify_slice(&entries, &seed));
}
