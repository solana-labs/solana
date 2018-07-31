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

echo --- ci/localnet-sanity.sh
(
  set -x
  # Assume |cargo build| has populated target/debug/ successfully.
  export PATH=$PWD/target/debug:$PATH
  USE_INSTALL=1 ci/localnet-sanity.sh
)

_ ci/audit.sh
