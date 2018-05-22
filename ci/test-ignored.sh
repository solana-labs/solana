#!/bin/bash -e

cd $(dirname $0)/..

source $HOME/.cargo/env
rustup update
export RUST_BACKTRACE=1
cargo test -- --ignored
