#!/bin/bash -e

cd "$(dirname "$0")/.."

export RUST_BACKTRACE=1
rustc --version
cargo --version

_() {
  echo "--- $*"
  "$@"
}

_ rustup component add rustfmt-preview
_ cargo fmt -- --write-mode=check
_ cargo build --verbose
_ cargo test --verbose
_ cargo bench --verbose
