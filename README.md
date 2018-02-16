[![Build Status](https://travis-ci.org/garious/phist.svg?branch=master)](https://travis-ci.org/garious/phist)
[![codecov](https://codecov.io/gh/garious/phist/branch/master/graph/badge.svg)](https://codecov.io/gh/garious/phist)

# :punch: phist :punch:

An implementation of Loom's Proof-of-History.


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
$ git clone https://github.com/garious/phist.git
$ cd phist
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
