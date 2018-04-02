The Historian
===

Create a *Historian* and send it *events* to generate an *event log*, where each *entry*
is tagged with the historian's latest *hash*. Then ensure the order of events was not tampered
with by verifying each entry's hash can be generated from the hash in the previous entry:

![historian](https://user-images.githubusercontent.com/55449/36950845-459bdb58-1fb9-11e8-850e-894586f3729b.png)

```rust
extern crate solana;

use solana::historian::Historian;
use solana::ledger::{Block, Entry, Hash};
use solana::event::{generate_keypair, get_pubkey, sign_claim_data, Event};
use std::thread::sleep;
use std::time::Duration;
use std::sync::mpsc::SendError;

fn create_ledger(hist: &Historian<Hash>) -> Result<(), SendError<Event<Hash>>> {
    sleep(Duration::from_millis(15));
    let tokens = 42;
    let keypair = generate_keypair();
    let event0 = Event::new_claim(get_pubkey(&keypair), tokens, sign_claim_data(&tokens, &keypair));
    hist.sender.send(event0)?;
    sleep(Duration::from_millis(10));
    Ok(())
}

fn main() {
    let seed = Hash::default();
    let hist = Historian::new(&seed, Some(10));
    create_ledger(&hist).expect("send error");
    drop(hist.sender);
    let entries: Vec<Entry<Hash>> = hist.receiver.iter().collect();
    for entry in &entries {
        println!("{:?}", entry);
    }
    // Proof-of-History: Verify the historian learned about the events
    // in the same order they appear in the vector.
    assert!(entries[..].verify(&seed));
}
```

Running the program should produce a ledger similar to:

```rust
Entry { num_hashes: 0, id: [0, ...], event: Tick }
Entry { num_hashes: 3, id: [67, ...], event: Transaction { tokens: 42 } }
Entry { num_hashes: 3, id: [123, ...], event: Tick }
```

Proof-of-History
---

Take note of the last line:

```rust
assert!(entries[..].verify(&seed));
```

[It's a proof!](https://en.wikipedia.org/wiki/Curry–Howard_correspondence) For each entry returned by the
historian, we can verify that `id` is the result of applying a sha256 hash to the previous `id`
exactly `num_hashes` times, and then hashing then event data on top of that. Because the event data is
included in the hash, the events cannot be reordered without regenerating all the hashes.
