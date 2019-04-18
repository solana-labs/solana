#!/usr/bin/env bash

here=$(dirname "$0")
# shellcheck source=multinode-demo/common.sh
source "$here"/common.sh

lamports=100000000000000
bootstrap_leader_lamports=

usage () {
  exitcode=0
  if [[ -n "$1" ]]; then
    exitcode=1
    echo "Error: $*"
  fi
  cat <<EOF
usage: $0 [-n lamports] [-b lamports]

Create a cluster configuration

 -n lamports    - Number of lamports to create [default: $lamports]
 -b lamports    - Override the number of lamports for the bootstrap leader's stake

EOF
  exit $exitcode
}

while getopts "h?n:b:" opt; do
  case $opt in
  h|\?)
    usage
    exit 0
    ;;
  n)
    lamports="$OPTARG"
    ;;
  b)
    bootstrap_leader_lamports="$OPTARG"
    ;;
  *)
    usage "Error: unhandled option: $opt"
    ;;
  esac
done


set -e
"$here"/clear-fullnode-config.sh

# Create genesis ledger
$solana_keygen -o "$SOLANA_CONFIG_DIR"/mint-id.json
$solana_keygen -o "$SOLANA_CONFIG_DIR"/bootstrap-leader-id.json
$solana_keygen -o "$SOLANA_CONFIG_DIR"/bootstrap-leader-vote-id.json

args=(
  --bootstrap-leader-keypair "$SOLANA_CONFIG_DIR"/bootstrap-leader-id.json
  --bootstrap-vote-keypair "$SOLANA_CONFIG_DIR"/bootstrap-leader-vote-id.json
  --ledger "$SOLANA_RSYNC_CONFIG_DIR"/ledger
  --mint "$SOLANA_CONFIG_DIR"/mint-id.json
  --lamports "$lamports"
)

if [[ -n $bootstrap_leader_lamports ]]; then
  args+=(--bootstrap-leader-lamports "$bootstrap_leader_lamports")
fi

$solana_genesis "${args[@]}"
cp -a "$SOLANA_RSYNC_CONFIG_DIR"/ledger "$SOLANA_CONFIG_DIR"/bootstrap-leader-ledger
