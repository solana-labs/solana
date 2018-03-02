extern crate silk;

use silk::historian::Historian;
use silk::log::{generate_keypair, get_pubkey, sign_serialized, verify_slice, Entry, Event,
                Sha256Hash};
use std::thread::sleep;
use std::time::Duration;
use std::sync::mpsc::SendError;

fn create_log(hist: &Historian<Sha256Hash>) -> Result<(), SendError<Event<Sha256Hash>>> {
    sleep(Duration::from_millis(15));
    let data = Sha256Hash::default();
    let keypair = generate_keypair();
    let event0 = Event::Claim {
        key: get_pubkey(&keypair),
        data,
        sig: sign_serialized(&data, &keypair),
    };
    hist.sender.send(event0)?;
    sleep(Duration::from_millis(10));
    Ok(())
}

fn main() {
    let seed = Sha256Hash::default();
    let hist = Historian::new(&seed, Some(10));
    create_log(&hist).expect("send error");
    drop(hist.sender);
    let entries: Vec<Entry<Sha256Hash>> = hist.receiver.iter().collect();
    for entry in &entries {
        println!("{:?}", entry);
    }
    assert!(verify_slice(&entries, &seed));
}
