#!/usr/bin/env bash
set -e

FEATURES="$1"

cd "$(dirname "$0")/.."

# Clear cached json keypair files
rm -rf "$HOME/.config/solana"

ci/version-check-with-upgrade.sh stable
export RUST_BACKTRACE=1
export RUSTFLAGS="-D warnings"

_() {
  echo "--- $*"
  "$@"
}

_ scripts/ulimit-n.sh
_ cargo build --all --verbose --features="$FEATURES"
_ cargo test --all --verbose - --features="$FEATURES" --lib -- --nocapture --test-threads=1

# Run native program tests (without $FEATURES)
for program in programs/native/*; do
  echo --- "$program" test
  (
    set -x
    cd "$program"
    cargo test --verbose -- --nocapture
  )
done

# Run integration tests serially
for test in tests/*.rs; do
  test=${test##*/} # basename x
  test=${test%.rs} # basename x .rs
  _ cargo test --verbose --features="$FEATURES" --test="$test" -- --test-threads=1 --nocapture
done

echo --- ci/localnet-sanity.sh
(
  set -x
  # Assume |cargo build| has populated target/debug/ successfully.
  export PATH=$PWD/target/debug:$PATH
  USE_INSTALL=1 ci/localnet-sanity.sh
)
