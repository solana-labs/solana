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
corresponding benchmarks are also added that demonstrate real performance boots. We expect the
feature set here will always be a ways behind the loom repo, but that this is an implementation
you can take to the bank, literally.

# Usage

Add the latest [silk package](https://crates.io/crates/silk) to the `[dependencies]` section
of your Cargo.toml.

Create a *Historian* and send it *events* to generate an *event log*, where each log *entry*
is tagged with the historian's latest *hash*. Then ensure the order of events was not tampered
with by verifying each entry's hash can be generated from the hash in the previous entry:

```rust
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
```

Running the program should produce a log similar to:

```
Entry { num_hashes: 0, end_hash: 0, event: Tick }
Entry { num_hashes: 245, end_hash: 11504657626326377539, event: UserDataKey(3735928559) }
Entry { num_hashes: 154, end_hash: 13410333856574024888, event: Tick }
```


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
$ cargo +nightly bench --features="unstable"
```
