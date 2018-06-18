#!/bin/bash -e

cd "$(dirname "$0")/.."

rustc --version
cargo --version

export RUST_BACKTRACE=1
rustup component add rustfmt-preview
cargo build --verbose --features unstable
cargo test --verbose --features unstable
cargo bench --verbose --features unstable
ci/coverage.sh

exit 0
