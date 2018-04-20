[![Solana crate](https://img.shields.io/crates/v/solana.svg)](https://crates.io/crates/solana)
[![Solana documentation](https://docs.rs/solana/badge.svg)](https://docs.rs/solana)
[![Build Status](https://travis-ci.org/solana-labs/solana.svg?branch=master)](https://travis-ci.org/solana-labs/solana)
[![codecov](https://codecov.io/gh/solana-labs/solana/branch/master/graph/badge.svg)](https://codecov.io/gh/solana-labs/solana)

Disclaimer
===

All claims, content, designs, algorithms, estimates, roadmaps, specifications, and performance measurements described in this project are done with the author's best effort.  It is up to the reader to check and validate their accuracy and truthfulness.  Furthermore nothing in this project constitutes a solicitation for investment.

Solana: High Performance Blockchain
===

Solana&trade; is a new architecture for a high performance blockchain. It aims to support
over 700 thousand transactions per second on a gigabit network.

Introduction
===

It's possible for a centralized database to process 710,000 transactions per second on a standard gigabit network if the transactions are, on average, no more than 178 bytes. A centralized database can also replicate itself and maintain high availability without significantly compromising that transaction rate using the distributed system technique known as Optimistic Concurrency Control [H.T.Kung, J.T.Robinson (1981)]. At Solana, we're demonstrating that these same theoretical limits apply just as well to blockchain on an adversarial network. The key ingredient? Finding a way to share time when nodes can't trust one-another. Once nodes can trust time, suddenly ~40 years of distributed systems research becomes applicable to blockchain! Furthermore, and much to our surprise, it can implemented using a mechanism that has existed in Bitcoin since day one. The Bitcoin feature is called nLocktime and it can be used to postdate transactions using block height instead of a timestamp. As a Bitcoin client, you'd use block height instead of a timestamp if you don't trust the network. Block height turns out to be an instance of what's being called a Verifiable Delay Function in cryptography circles. It's a cryptographically secure way to say time has passed. In Solana, we use a far more granular verifiable delay function, a SHA 256 hash chain, to checkpoint the ledger and coordinate consensus. With it, we implement Optimistic Concurrency Control and are now well in route towards that theoretical limit of 710,000 transactions per second.

Running the demo
===

First, install Rust's package manager Cargo.

```bash
$ curl https://sh.rustup.rs -sSf | sh
$ source $HOME/.cargo/env
```

Now checkout the code from github:

```bash
$ git clone https://github.com/solana-labs/solana.git 
$ cd solana
```

The testnode server is initialized with a ledger from stdin and
generates new ledger entries on stdout. To create the input ledger, we'll need
to create *the mint* and use it to generate a *genesis ledger*. It's done in
two steps because the mint.json file contains a private key that will be
used later in this demo.

```bash
    $ echo 1000000000 | cargo run --release --bin solana-mint | tee mint.json
    $ cat mint.json | cargo run --release --bin solana-genesis | tee genesis.log
```

Now you can start the server:

```bash
    $ cat genesis.log | cargo run --release --bin solana-testnode | tee transactions0.log
```

Then, in a separate shell, let's execute some transactions. Note we pass in
the JSON configuration file here, not the genesis ledger.

```bash
    $ cat mint.json | cargo run --release --bin solana-client-demo
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
    $ cat genesis.log transactions0.log | cargo run --release --bin solana-testnode | tee transactions1.log
```

Lastly, run the client demo again, and verify that all funds were spent in the
previous round, and so no additional transactions are added.

```bash
    $ cat mint.json | cargo run --release --bin solana-client-demo
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

Code coverage
---

To generate code coverage statistics, run kcov via Docker:

```bash
$ docker run -it --rm --security-opt seccomp=unconfined --volume "$PWD:/volume" elmtai/docker-rust-kcov
```

Why coverage? While most see coverage as a code quality metric, we see it primarily as a developer
productivity metric. When a developer makes a change to the codebase, presumably it's a *solution* to
some problem.  Our unit-test suite is how we encode the set of *problems* the codebase solves. Running
the test suite should indicate that your change didn't *infringe* on anyone else's solutions. Adding a
test *protects* your solution from future changes. Say you don't understand why a line of code exists,
try deleting it and running the unit-tests. The nearest test failure should tell you what problem
was solved by that code. If no test fails, go ahead and submit a Pull Request that asks, "what
problem is solved by this code?" On the other hand, if a test does fail and you can think of a
better way to solve the same problem, a Pull Request with your solution would most certainly be
welcome! Likewise, if rewriting a test can better communicate what code it's protecting, please
send us that patch!
