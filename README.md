[![Silk crate](https://img.shields.io/crates/v/silk.svg)](https://crates.io/crates/silk)
[![Silk documentation](https://docs.rs/silk/badge.svg)](https://docs.rs/silk)
[![Build Status](https://travis-ci.org/loomprotocol/silk.svg?branch=master)](https://travis-ci.org/loomprotocol/silk)
[![codecov](https://codecov.io/gh/loomprotocol/silk/branch/master/graph/badge.svg)](https://codecov.io/gh/loomprotocol/silk)

Disclaimer
===

All claims, content, designs, algorithms, estimates, roadmaps, specifications, and performance measurements described in this project are done with the author's best effort.  It is up to the reader to check and validate their accuracy and truthfulness.  Furthermore nothing in this project constitutes a solicitation for investment.

Silk, a silky smooth implementation of the Loom specification
===

Loom&trade; is a new architecture for a high performance blockchain. Its white paper boasts a theoretical
throughput of 710k transactions per second on a 1 gbps network. The specification is implemented
in two git repositories. Research is performed in the loom repository. That work drives the
Loom specification forward. This repository, on the other hand, aims to implement the specification
as-is.  We care a great deal about quality, clarity and short learning curve. We avoid the use
of `unsafe` Rust and write tests for *everything*.  Optimizations are only added when
corresponding benchmarks are also added that demonstrate real performance boosts. We expect the
feature set here will always be a ways behind the loom repo, but that this is an implementation
you can take to the bank, literally.

Running the demo
===

First, build the demo executables in release mode (optimized for performance):

```bash
    $ cargo build --release
    $ cd target/release
```

The testnode server is initialized with a transaction log from stdin and
generates a log on stdout. To create the input log, we'll need to create
a *genesis* configuration file and then generate a log from it. It's done
in two steps here because the demo-genesis.json file contains a private
key that will be used later in this demo.

```bash
    $ ./silk-genesis-file-demo > demo-genesis.jsoc
    $ cat demo-genesis.json | ./silk-genesis-block > demo-genesis.log
```

Now you can start the server:

```bash
    $ cat demo-genesis.log | ./silk-testnode > demo-entries0.log
```

Then, in a separate shell, let's execute some transactions. Note we pass in
the JSON configuration file here, not the genesis log.

```bash
    $ cat demo-genesis.json | ./silk-client-demo
```

Now kill the server with Ctrl-C, and take a look at the transaction log. You should
see something similar to:

```json
{"num_hashes":27,"id":[0, "..."],"event":"Tick"}
{"num_hashes":3,"id":[67, "..."],"event":{"Transaction":{"asset":42}}}
{"num_hashes":27,"id":[0, "..."],"event":"Tick"}
```

Now restart the server from where we left off. Pass it both the genesis log, and
the transaction log.

```bash
    $ cat demo-genesis.log demo-entries0.log | ./silk-testnode > demo-entries1.log
```

Lastly, run the client demo again, and verify that all funds were spent in the
previous round, and so no additional transactions are added.

```bash
    $ cat demo-genesis.json | ./silk-client-demo
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
