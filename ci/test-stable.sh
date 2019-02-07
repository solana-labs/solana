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

source scripts/ulimit-n.sh
maybeFeatures=
if [[ -n $FEATURES ]]; then
  maybeFeatures="--features=$FEATURES"
fi
# shellcheck disable=SC2086 # Don't want to double quote $maybeFeatures
_ cargo build --all ${V:+--verbose} $maybeFeatures
# shellcheck disable=SC2086 # Don't want to double quote $maybeFeatures
_ cargo test --all ${V:+--verbose} $maybeFeatures --lib -- --nocapture --test-threads=1

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
