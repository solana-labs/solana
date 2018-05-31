[![Solana crate](https://img.shields.io/crates/v/solana.svg)](https://crates.io/crates/solana)
[![Solana documentation](https://docs.rs/solana/badge.svg)](https://docs.rs/solana)
[![Build status](https://badge.buildkite.com/d4c4d7da9154e3a8fb7199325f430ccdb05be5fc1e92777e51.svg?branch=master)](https://buildkite.com/solana-labs/solana)
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

The fullnode server is initialized with a ledger from stdin and
generates new ledger entries on stdout. To create the input ledger, we'll need
to create *the mint* and use it to generate a *genesis ledger*. It's done in
two steps because the mint-demo.json file contains private keys that will be
used later in this demo.

```bash
$ echo 1000000000 | cargo run --release --bin solana-mint-demo > mint-demo.json
$ cat mint-demo.json | cargo run --release --bin solana-genesis-demo > genesis.log
```

Before you start the server, make sure you know the IP address of the machine you
want to be the leader for the demo, and make sure that udp ports 8000-10000 are
open on all the machines you want to test with.

Generate a leader configuration file with:

```bash
cargo run --release --bin solana-fullnode-config > leader.json
```

Now start the server:

```bash
$ cat ./multinode-demo/leader.sh
#!/bin/bash
export RUST_LOG=solana=info
sudo sysctl -w net.core.rmem_max=26214400
cat genesis.log | cargo run --release --bin solana-fullnode -- -l leader.json
$ ./multinode-demo/leader.sh > leader-txs.log
```

Wait a few seconds for the server to initialize. It will print "Ready." when it's safe
to start sending it transactions.

Now you can start some validators:

```bash
$ cat ./multinode-demo/validator.sh
#!/bin/bash
rsync -v -e ssh $1/mint-demo.json .
rsync -v -e ssh $1/leader.json .
rsync -v -e ssh $1/genesis.log .
export RUST_LOG=solana=info
sudo sysctl -w net.core.rmem_max=26214400
cat genesis.log | cargo run --release --bin solana-fullnode -- -l validator.json -v leader.json -b 9000 -d
$ ./multinode-demo/validator.sh ubuntu@10.0.1.51:~/solana > validator-txs.log #The leader machine
```


Then, in a separate shell, let's execute some transactions. Note we pass in
the JSON configuration file here, not the genesis ledger.

```bash
$ cat ./multinode-demo/client.sh
#!/bin/bash
export RUST_LOG=solana=info
rsync -v -e ssh $1/leader.json .
rsync -v -e ssh $1/mint-demo.json .
cat mint-demo.json | cargo run --release --bin solana-client-demo -- -l leader.json -c 8100 -n 1
$ ./multinode-demo/client.sh ubuntu@10.0.1.51:~/solana #The leader machine
```

Try starting a more validators and reruning the client demo and change the `-n 1` option in `client.sh`!

To enable cuda, downlaod the cuda library (see #Benchmarking) and add `--features=cuda` to the leader and validator scripts (`--release --features=cuda`).

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

If your rustc version is lower than 1.26.1, please update it:

```bash
$ rustup update
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
$ cargo test
```

To emulate all the tests that will run on a Pull Request, run:
```bash
$ ./ci/run-local.sh
```

Debugging
---

There are some useful debug messages in the code, you can enable them on a per-module and per-level
basis with the normal RUST\_LOG environment variable. Run the fullnode with this syntax:
```bash
$ RUST_LOG=solana::streamer=debug,solana::server=info cat genesis.log | ./target/release/solana-fullnode > transactions0.log
```
to see the debug and info sections for streamer and server respectively. Generally
we are using debug for infrequent debug messages, trace for potentially frequent messages and
info for performance-related logging.

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

To run the benchmarks on Linux with GPU optimizations enabled:

```bash
$ wget https://solana-build-artifacts.s3.amazonaws.com/v0.6.0/libcuda_verify_ed25519.a
$ cargo +nightly bench --features="unstable,cuda"
```

Code coverage
---

To generate code coverage statistics, run kcov via Docker:

```bash
$ ./ci/coverage.sh
```
The coverage report will be written to `./target/cov/index.html`


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
