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

Add the latest [silk package] (https://crates.io/crates/silk) to the `[dependencies]` section
of your Cargo.toml.

Create a *Historian* and send it *events* to generate an *event log*, where each log *entry*
is tagged with the historian's latest *hash*. Then ensure the order of events was not tampered
with by verifying each entry's hash can be generated from the hash in the previous entry:

```rust
use historian::Historian;
use log::{Event, verify_slice};

fn main() {
    let hist = Historian::new(0);

    hist.sender.send(Event::Tick).unwrap();
    let entry0 = hist.receiver.recv().unwrap();

    hist.sender.send(Event::UserDataKey(0xdeadbeef)).unwrap();
    let entry1 = hist.receiver.recv().unwrap();

    assert!(verify_slice(&[entry0, entry1], 0));
}
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
