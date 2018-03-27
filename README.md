[![Solana crate](https://img.shields.io/crates/v/solana.svg)](https://crates.io/crates/solana)
[![Solana documentation](https://docs.rs/solana/badge.svg)](https://docs.rs/solana)
[![Build Status](https://travis-ci.org/solana-labs/solana.svg?branch=master)](https://travis-ci.org/solana-labs/solana)
[![codecov](https://codecov.io/gh/solana-labs/solana/branch/master/graph/badge.svg)](https://codecov.io/gh/solana-labs/solana)

Disclaimer
===

All claims, content, designs, algorithms, estimates, roadmaps, specifications, and performance measurements described in this project are done with the author's best effort.  It is up to the reader to check and validate their accuracy and truthfulness.  Furthermore nothing in this project constitutes a solicitation for investment.

Solana: High-Performance Blockchain
===

Solana&trade; is a new architecture for a high performance blockchain. It aims to support
over 700 thousand transactions per second on a gigabit network.

Running the demo
===

First, install Rust's package manager Cargo.

```bash
$ curl https://sh.rustup.rs -sSf | sh
$ source $HOME/.cargo/env
```

Install the solana executables:

```bash
    $ cargo install solana
```

The testnode server is initialized with a ledger from stdin and
generates new ledger entries on stdout. To create the input ledger, we'll need
to create *the mint* and use it to generate a *genesis ledger*. It's done in
two steps because the mint.json file contains a private key that will be
used later in this demo.

```bash
    $ echo 500 | solana-mint > mint.json
    $ cat mint.json | solana-genesis > genesis.log
```

Now you can start the server:

```bash
    $ cat genesis.log | solana-testnode > transactions0.log
```

Then, in a separate shell, let's execute some transactions. Note we pass in
the JSON configuration file here, not the genesis ledger.

```bash
    $ cat mint.json | solana-client-demo
```

Now kill the server with Ctrl-C, and take a look at the ledger. You should
see something similar to:

```json
{"num_hashes":27,"id":[0, "..."],"event":"Tick"}
{"num_hashes":3,"id":[67, "..."],"event":{"Transaction":{"tokens":42}}}
{"num_hashes":27,"id":[0, "..."],"event":"Tick"}
```

Now restart the server from where we left off. Pass it both the genesis ledger, and
the transaction ledger.

```bash
    $ cat genesis.log transactions0.log | solana-testnode > transactions1.log
```

Lastly, run the client demo again, and verify that all funds were spent in the
previous round, and so no additional transactions are added.

```bash
    $ cat mint.json | solana-client-demo
```

Stop the server again, and verify there are only Tick entries, and no Transaction entries.

Developing
===

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
$ git clone https://github.com/solana-labs/solana.git
$ cd solana
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
