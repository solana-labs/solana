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
_ cargo build --all --features="$FEATURES"

export PATH=$PWD/target/debug:$PATH
export USE_INSTALL=1

# Leader rotation disabled
_ ci/localnet-sanity.sh -b -i 128

# Leader rotation disabled, restart all nodes periodically
_ ci/localnet-sanity.sh -b -i 128 -k 16

# Leader rotation enabled
_ ci/localnet-sanity.sh -i 128

# Leader rotation enabled, restart all nodes periodically
# TODO: Enable
#_ ci/localnet-sanity.sh -i 128 -k 16

echo --- fin
