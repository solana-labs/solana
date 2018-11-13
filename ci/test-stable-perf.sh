#!/usr/bin/env bash
set -e

cd "$(dirname "$0")/.."

# Clear cached json keypair files
rm -rf "$HOME/.config/solana"

if ! ci/version-check.sh stable; then
  # This job doesn't run within a container, try once to upgrade tooling on a
  # version check failure
  rustup install stable
  ci/version-check.sh stable
fi
export RUST_BACKTRACE=1
export RUSTFLAGS="-D warnings"

./fetch-perf-libs.sh
# shellcheck source=/dev/null
source ./target/perf-libs/env.sh

_() {
  echo "--- $*"
  "$@"
}

FEATURES=cuda,erasure,chacha
_ cargo test --verbose --features="$FEATURES" --lib

# Run integration tests serially
for test in tests/*.rs; do
  test=${test##*/} # basename x
  test=${test%.rs} # basename x .rs
  _ cargo test --verbose --jobs=1 --features="$FEATURES" --test="$test"
done

echo --- ci/localnet-sanity.sh
(
  set -x
  # Assume |cargo build| has populated target/debug/ successfully.
  export PATH=$PWD/target/debug:$PATH
  USE_INSTALL=1 ci/localnet-sanity.sh
)
