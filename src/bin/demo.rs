extern crate silk;

use silk::historian::Historian;
use silk::log::{verify_slice, Entry, Event};
use std::{thread, time};

fn create_log(hist: &Historian) -> Vec<Entry> {
    hist.sender.send(Event::Tick).unwrap();
    thread::sleep(time::Duration::new(0, 100_000));
    hist.sender.send(Event::UserDataKey(0xdeadbeef)).unwrap();
    thread::sleep(time::Duration::new(0, 100_000));
    hist.sender.send(Event::Tick).unwrap();

    let entry0 = hist.receiver.recv().unwrap();
    let entry1 = hist.receiver.recv().unwrap();
    let entry2 = hist.receiver.recv().unwrap();
    vec![entry0, entry1, entry2]
}

fn main() {
    let seed = 0;
    let hist = Historian::new(seed);
    let entries = create_log(&hist);
    for entry in &entries {
        println!("{:?}", entry);
    }
    assert!(verify_slice(&entries, seed));
}
