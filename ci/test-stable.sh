#!/bin/bash -e

cd "$(dirname "$0")/.."

ci/version-check.sh stable
export RUST_BACKTRACE=1

_() {
  echo "--- $*"
  "$@"
}

_ cargo fmt -- --check
_ cargo build --verbose
_ cargo test --verbose

echo --- ci/localnet-sanity.sh
(
  set -x
  # Assume |cargo build| has populated target/debug/ successfully.
  export PATH=$PWD/target/debug:$PATH
  USE_INSTALL=1 ci/localnet-sanity.sh
)

_ ci/audit.sh || true
