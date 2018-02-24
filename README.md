[![Silk crate](https://img.shields.io/crates/v/silk.svg)](https://crates.io/crates/silk)
[![Silk documentation](https://docs.rs/silk/badge.svg)](https://docs.rs/silk)
[![Build Status](https://travis-ci.org/loomprotocol/silk.svg?branch=master)](https://travis-ci.org/loomprotocol/silk)
[![codecov](https://codecov.io/gh/loomprotocol/silk/branch/master/graph/badge.svg)](https://codecov.io/gh/loomprotocol/silk)

# Silk, a silky smooth implementation of the Loom specification

Loom is a new achitecture for a high performance blockchain. Its whitepaper boasts a theoretical
throughput of 710k transactions per second on a 1 gbps network. The specification is implemented
in two git repositories. Reserach is performed in the loom repository. That work drives the
Loom specification forward. This repository, on the other hand, aims to implement the specification
as-is.  We care a great deal about quality, clarity and short learning curve. We avoid the use
of `unsafe` Rust and write tests for *everything*.  Optimizations are only added when
corresponding benchmarks are also added that demonstrate real performance boosts. We expect the
feature set here will always be a ways behind the loom repo, but that this is an implementation
you can take to the bank, literally.

# Usage

Add the latest [silk package](https://crates.io/crates/silk) to the `[dependencies]` section
of your Cargo.toml.

Create a *Historian* and send it *events* to generate an *event log*, where each log *entry*
is tagged with the historian's latest *hash*. Then ensure the order of events was not tampered
with by verifying each entry's hash can be generated from the hash in the previous entry:

![historian](https://user-images.githubusercontent.com/55449/36633440-f76f7bb8-1952-11e8-8328-387861d3d464.png)

```rust
extern crate silk;

use silk::historian::Historian;
use silk::log::{verify_slice, Entry, Event, Sha256Hash};
use std::thread::sleep;
use std::time::Duration;
use std::sync::mpsc::SendError;

fn create_log(hist: &Historian) -> Result<(), SendError<Event>> {
    sleep(Duration::from_millis(15));
    hist.sender.send(Event::Discovery(Sha256Hash::default()))?;
    sleep(Duration::from_millis(10));
    Ok(())
}

fn main() {
    let seed = Sha256Hash::default();
    let hist = Historian::new(&seed, Some(10));
    create_log(&hist).expect("send error");
    drop(hist.sender);
    let entries: Vec<Entry> = hist.receiver.iter().collect();
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
Entry { num_hashes: 0, end_hash: [0, ...], event: Tick }
Entry { num_hashes: 2, end_hash: [67, ...], event: Discovery(3735928559) }
Entry { num_hashes: 3, end_hash: [123, ...], event: Tick }
```

Proof-of-History
---

Take note of the last line:

```rust
assert!(verify_slice(&entries, &seed));
```

[It's a proof!](https://en.wikipedia.org/wiki/Curry–Howard_correspondence) For each entry returned by the
historian, we can verify that `end_hash` is the result of applying a sha256 hash to the previous `end_hash`
exactly `num_hashes` times, and then hashing then event data on top of that. Because the event data is
included in the hash, the events cannot be reordered without regenerating all the hashes.

# Developing

Building
---

Install rustc, cargo and rustfmt:

```bash
$ curl https://sh.rustup.rs -sSf | sh
$ source $HOME/.cargo/env
$ rustup component add rustfmt-preview
```

Download the source code:

```bash
$ git clone https://github.com/loomprotocol/silk.git
$ cd silk
```

Testing
---

Run the test suite:

```bash
cargo test
```

Benchmarking
---

First install the nightly build of rustc. `cargo bench` requires unstable features:

```bash
$ rustup install nightly
```

Run the benchmarks:

```bash
$ cargo +nightly bench --features="asm,unstable"
```
