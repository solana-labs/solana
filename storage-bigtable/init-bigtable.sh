#!/usr/bin/env bash
#
# Configures a BigTable instance with the expected tables
#

set -e

instance=solana-ledger

cbt=(
  cbt
  -instance
  "$instance"
)
if [[ -n $BIGTABLE_EMULATOR_HOST ]]; then
  cbt+=(-project emulator)
fi

for table in blocks tx tx-by-addr; do
  (
    set -x
    "${cbt[@]}" createtable $table
    "${cbt[@]}" createfamily $table x
    "${cbt[@]}" setgcpolicy $table x maxversions=1
    "${cbt[@]}" setgcpolicy $table x maxage=360d
  )
done
