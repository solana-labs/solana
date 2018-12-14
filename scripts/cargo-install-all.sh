#!/usr/bin/env bash
#
# |cargo install| of the top-level crate will not install binaries for
# other workspace creates.
set -e
cd "$(dirname "$0")/.."

SECONDS=0

CRATES=(
  drone
  keygen
  fullnode
  bench-streamer
  bench-tps
  fullnode-config
  genesis
  ledger-tool
  wallet
)
for crate in "${CRATES[@]}"; do
  (
    set -x
    cargo install --path "$crate" "$@"
  )
done

echo "Done after $SECONDS seconds"
