#!/usr/bin/env bash
set -e
FEATURES="$1"
cd "$(dirname "$0")/.."

# Clear cached json keypair files
rm -rf "$HOME/.config/solana"

source ci/_
ci/version-check-with-upgrade.sh stable
export RUST_BACKTRACE=1
export RUSTFLAGS="-D warnings"

_ scripts/ulimit-n.sh
_ cargo build --all --verbose --features="$FEATURES"
_ cargo test --all --verbose --features="$FEATURES" --lib -- --nocapture --test-threads=1

# Run native program tests (without $FEATURES)
for program in programs/native/*; do
  echo --- "$program" test
  (
    set -x
    cd "$program"
    cargo test --verbose -- --nocapture
  )
done

_ ci/integration-tests.sh "$FEATURES"

echo --- ci/localnet-sanity.sh
(
  set -x
  # Assume |cargo build| has populated target/debug/ successfully.
  export PATH=$PWD/target/debug:$PATH
  USE_INSTALL=1 ci/localnet-sanity.sh -x
)
