#!/usr/bin/env bash
set -e

cd "$(dirname "$0")/.."

ci/version-check.sh stable
export RUST_BACKTRACE=1
export RUSTFLAGS="-D warnings"

_() {
  echo "--- $*"
  "$@"
}

if [[ $(ulimit -n) -lt 65000 ]]; then
  echo 'Error: nofiles too small, run "ulimit -n 65000" to continue'
  exit 1
fi

_ cargo build --all --verbose
_ cargo test --verbose --lib -- --nocapture

# Run integration tests serially
for test in tests/*.rs; do
  test=${test##*/} # basename x
  test=${test%.rs} # basename x .rs
  _ cargo test --verbose --test="$test" -- --test-threads=1 --nocapture
done

# Run native program tests
for program in programs/native/*; do
  echo --- "$program"
  (
    set -x
    cd "$program"
    cargo test --verbose -- --nocapture
  )
done

book/build.sh

echo --- ci/localnet-sanity.sh
(
  set -x
  # Assume |cargo build| has populated target/debug/ successfully.
  export PATH=$PWD/target/debug:$PATH
  USE_INSTALL=1 ci/localnet-sanity.sh
)
