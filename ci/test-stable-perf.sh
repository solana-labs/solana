#!/usr/bin/env bash
set -e

cd "$(dirname "$0")/.."

# Clear cached json keypair files
rm -rf "$HOME/.config/solana"

# This job doesn't run within a container, try once to upgrade tooling on a
# version check failure
ci/version-check-with-upgrade.sh stable

export RUST_BACKTRACE=1
export RUSTFLAGS="-D warnings"

_() {
  echo "--- $*"
  "$@"
}

./fetch-perf-libs.sh
# shellcheck source=/dev/null
source ./target/perf-libs/env.sh

FEATURES=bpf_c,cuda,erasure,chacha
_ cargo build --all --verbose --features="$FEATURES"
_ cargo test --verbose --features="$FEATURES" --lib

# Run integration tests serially
for test in tests/*.rs; do
  test=${test##*/} # basename x
  test=${test%.rs} # basename x .rs
  _ cargo test --verbose --features="$FEATURES" --test="$test" -- --test-threads=1
done

# Run bpf_loader test with bpf_c features enabled
(
  set -x
  cd "programs/native/bpf_loader"
  echo --- program/native/bpf_loader test --features=bpf_c
  cargo test --verbose --features="bpf_c"
)

echo --- ci/localnet-sanity.sh
(
  set -x
  # Assume |cargo build| has populated target/debug/ successfully.
  export PATH=$PWD/target/debug:$PATH
  USE_INSTALL=1 ci/localnet-sanity.sh
)
