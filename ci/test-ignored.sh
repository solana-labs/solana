#!/bin/bash -e

cd "$(dirname "$0")/.."

rustc --version
cargo --version

export RUST_BACKTRACE=1
cargo test -- --ignored
