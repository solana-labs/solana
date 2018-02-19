extern crate silk;

use silk::historian::Historian;
use silk::log::{verify_slice, Entry, Event};
use std::{thread, time};
use std::sync::mpsc::SendError;

fn create_log(hist: &Historian) -> Result<(), SendError<Event>> {
    hist.sender.send(Event::Tick)?;
    thread::sleep(time::Duration::new(0, 100_000));
    hist.sender.send(Event::UserDataKey(0xdeadbeef))?;
    thread::sleep(time::Duration::new(0, 100_000));
    hist.sender.send(Event::Tick)?;
    Ok(())
}

fn main() {
    let seed = 0;
    let hist = Historian::new(seed);
    create_log(&hist).expect("send error");
    drop(hist.sender);
    let entries: Vec<Entry> = hist.receiver.iter().collect();
    for entry in &entries {
        println!("{:?}", entry);
    }
    assert!(verify_slice(&entries, seed));
}
