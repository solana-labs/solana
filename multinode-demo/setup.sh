#!/usr/bin/env bash
#
# Creates a fullnode configuration
#

here=$(dirname "$0")
# shellcheck source=multinode-demo/common.sh
source "$here"/common.sh

usage () {
  exitcode=0
  if [[ -n "$1" ]]; then
    exitcode=1
    echo "Error: $*"
  fi
  cat <<EOF
usage: $0 [-n lamports] [-l] [-p] [-t node_type]

Creates a fullnode configuration

 -n lamports    - Number of lamports to create
 -t node_type   - Create configuration files only for this kind of node.  Valid
                  options are bootstrap-leader or fullnode.  Creates configuration files
                  for both by default

EOF
  exit $exitcode
}

lamports=1000000000
bootstrap_leader=true
fullnode=true
while getopts "h?n:lpt:" opt; do
  case $opt in
  h|\?)
    usage
    exit 0
    ;;
  n)
    lamports="$OPTARG"
    ;;
  t)
    node_type="$OPTARG"
    case $OPTARG in
    bootstrap-leader|leader) # TODO: Remove legacy 'leader' option
      bootstrap_leader=true
      fullnode=false
      ;;
    fullnode|validator) # TODO: Remove legacy 'validator' option
      bootstrap_leader=false
      fullnode=true
      ;;
    *)
      usage "Error: unknown node type: $node_type"
      ;;
    esac
    ;;
  *)
    usage "Error: unhandled option: $opt"
    ;;
  esac
done


set -e

for i in "$SOLANA_RSYNC_CONFIG_DIR" "$SOLANA_CONFIG_DIR"; do
  echo "Cleaning $i"
  rm -rvf "$i"
  mkdir -p "$i"
done

if $bootstrap_leader; then
  # Create genesis configuration
  (
    set -x
    $solana_keygen -o "$SOLANA_CONFIG_DIR"/mint-id.json
    $solana_keygen -o "$SOLANA_CONFIG_DIR"/bootstrap-leader-id.json
    $solana_keygen -o "$SOLANA_CONFIG_DIR"/bootstrap-leader-staker-id.json
    $solana_genesis \
      --bootstrap-leader-keypair "$SOLANA_CONFIG_DIR"/bootstrap-leader-id.json \
      --bootstrap-stake-keypair "$SOLANA_CONFIG_DIR"/bootstrap-leader-staker-id.json \
      --ledger "$SOLANA_RSYNC_CONFIG_DIR"/ledger \
      --mint "$SOLANA_CONFIG_DIR"/mint-id.json \
      --lamports "$lamports"
    cp -a "$SOLANA_RSYNC_CONFIG_DIR"/ledger "$SOLANA_CONFIG_DIR"/bootstrap-leader-ledger
  )
fi

if $fullnode; then
  (
    set -x
    $solana_keygen -o "$SOLANA_CONFIG_DIR"/fullnode-id.json
    $solana_keygen -o "$SOLANA_CONFIG_DIR"/fullnode-staker-id.json
  )
fi
