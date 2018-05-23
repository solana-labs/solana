#!/bin/bash -e

cd $(dirname $0)/..

rustc --version
cargo --version

rustup component add rustfmt-preview
cargo fmt -- --write-mode=diff
cargo build --verbose
cargo test --verbose

exit 0
