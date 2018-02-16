[![Silk crate](https://img.shields.io/crates/v/silk.svg)](https://crates.io/crates/silk)
[![Silk documentation](https://docs.rs/silk/badge.svg)](https://docs.rs/silk)
[![Build Status](https://travis-ci.org/loomprotocol/silk.svg?branch=master)](https://travis-ci.org/loomprotocol/silk)
[![codecov](https://codecov.io/gh/loomprotocol/silk/branch/master/graph/badge.svg)](https://codecov.io/gh/loomprotocol/silk)

# Silk, A Silky Smooth Implementation of the Loom Architecture

Loom is a new achitecture for a high performance blockchain. Its whitepaper boasts a theoretical
throughput of 710k transactions per second on a 1 gbps network. The first implementation of the
whitepaper is happening in the 'loomprotocol/loom' repository. That repo is aggressively moving
forward, looking to de-risk technical claims as quickly as possible. This repo is quite a bit
different philosophically. Here we assume the Loom architecture is sound and worthy of building
a community around. We care a great deal about quality, clarity and short learning curve. We
avoid the use of `unsafe` Rust and an write tests for *everything*.  Optimizations are only
added when corresponding benchmarks are also added that demonstrate real performance boots. We
expect the feature set here will always be a long ways behind the loom repo, but that this is
an implementation you can take to the bank, literally.

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
