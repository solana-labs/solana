#!/bin/bash -e

cd "$(dirname "$0")/.."

ci/version-check.sh stable
export RUST_BACKTRACE=1
export RUSTFLAGS="-D warnings"

_() {
  echo "--- $*"
  "$@"
}

_ cargo fmt -- --check
_ cargo build --verbose
_ cargo test --verbose --lib
_ cargo clippy -- --deny=warnings

# Run integration tests serially
# shellcheck disable=SC2016
_ find tests -type file -exec sh -c 'echo --test=$(basename ${0%.*})' {} \; | xargs cargo test --verbose --jobs=1

echo --- ci/localnet-sanity.sh
(
  set -x
  # Assume |cargo build| has populated target/debug/ successfully.
  export PATH=$PWD/target/debug:$PATH
  USE_INSTALL=1 ci/localnet-sanity.sh
)

_ ci/audit.sh
