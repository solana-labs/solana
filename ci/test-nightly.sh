#!/bin/bash -e

cd $(dirname $0)/..

rustc --version
cargo --version

rustup component add rustfmt-preview
cargo build --verbose --features unstable
cargo test --verbose --features unstable
cargo bench --verbose --features unstable

exit 0
