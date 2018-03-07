The Historian
===

Create a *Historian* and send it *events* to generate an *event log*, where each log *entry*
is tagged with the historian's latest *hash*. Then ensure the order of events was not tampered
with by verifying each entry's hash can be generated from the hash in the previous entry:

![historian](https://user-images.githubusercontent.com/55449/36950845-459bdb58-1fb9-11e8-850e-894586f3729b.png)

```rust
extern crate silk;

use silk::historian::Historian;
use silk::log::{verify_slice, Entry, Hash};
use silk::event::{generate_keypair, get_pubkey, sign_claim_data, Event};
use std::thread::sleep;
use std::time::Duration;
use std::sync::mpsc::SendError;

fn create_log(hist: &Historian<Hash>) -> Result<(), SendError<Event<Hash>>> {
    sleep(Duration::from_millis(15));
    let asset = Hash::default();
    let keypair = generate_keypair();
    let event0 = Event::new_claim(get_pubkey(&keypair), asset, sign_claim_data(&asset, &keypair));
    hist.sender.send(event0)?;
    sleep(Duration::from_millis(10));
    Ok(())
}

fn main() {
    let seed = Hash::default();
    let hist = Historian::new(&seed, Some(10));
    create_log(&hist).expect("send error");
    drop(hist.sender);
    let entries: Vec<Entry<Hash>> = hist.receiver.iter().collect();
    for entry in &entries {
        println!("{:?}", entry);
    }
    // Proof-of-History: Verify the historian learned about the events
    // in the same order they appear in the vector.
    assert!(verify_slice(&entries, &seed));
}
```

Running the program should produce a log similar to:

```rust
Entry { num_hashes: 0, id: [0, ...], event: Tick }
Entry { num_hashes: 3, id: [67, ...], event: Transaction { asset: [37, ...] } }
Entry { num_hashes: 3, id: [123, ...], event: Tick }
```

Proof-of-History
---

Take note of the last line:

```rust
assert!(verify_slice(&entries, &seed));
```

[It's a proof!](https://en.wikipedia.org/wiki/Curry–Howard_correspondence) For each entry returned by the
historian, we can verify that `id` is the result of applying a sha256 hash to the previous `id`
exactly `num_hashes` times, and then hashing then event data on top of that. Because the event data is
included in the hash, the events cannot be reordered without regenerating all the hashes.
