#!/usr/bin/env bash
set -e
cd "$(dirname "$0")/.."

# Clear cached json keypair files
rm -rf "$HOME/.config/solana"

source ci/_
ci/version-check-with-upgrade.sh stable
export RUST_BACKTRACE=1
export RUSTFLAGS="-D warnings"

source scripts/ulimit-n.sh

_ cargo build --all ${V:+--verbose}
_ cargo test --all ${V:+--verbose} -- --nocapture --test-threads=1

echo --- ci/localnet-sanity.sh
(
  set -x
  # Assume |cargo build| has populated target/debug/ successfully.
  export PATH=$PWD/target/debug:$PATH
  USE_INSTALL=1 ci/localnet-sanity.sh -x
)
