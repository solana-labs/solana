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
usage: $0 [-n num_tokens] [-l] [-p] [-t node_type]

Creates a fullnode configuration

 -n num_tokens  - Number of tokens to create
 -l             - Detect network address from local machine configuration, which
                  may be a private IP address unaccessible on the Intenet (default)
 -p             - Detect public address using public Internet servers
 -t node_type   - Create configuration files only for this kind of node.  Valid
                  options are bootstrap-leader or fullnode.  Creates configuration files
                  for both by default

EOF
  exit $exitcode
}

ip_address_arg=-l
num_tokens=1000000000
bootstrap_leader=true
fullnode=true
while getopts "h?n:lpt:" opt; do
  case $opt in
  h|\?)
    usage
    exit 0
    ;;
  l)
    ip_address_arg=-l
    ;;
  p)
    ip_address_arg=-p
    ;;
  n)
    num_tokens="$OPTARG"
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
    $solana_genesis \
      --bootstrap-leader-keypair "$SOLANA_CONFIG_DIR"/bootstrap-leader-id.json \
      --ledger "$SOLANA_RSYNC_CONFIG_DIR"/ledger \
      --mint "$SOLANA_CONFIG_DIR"/mint-id.json \
      --num_tokens "$num_tokens"
  )

  # Create bootstrap leader configuration
  (
    set -x
    $solana_fullnode_config \
      --keypair="$SOLANA_CONFIG_DIR"/bootstrap-leader-id.json \
      "$ip_address_arg" > "$SOLANA_CONFIG_DIR"/bootstrap-leader.json

    cp -a "$SOLANA_RSYNC_CONFIG_DIR"/ledger "$SOLANA_CONFIG_DIR"/bootstrap-leader-ledger
  )
fi


if $fullnode; then
  (
    set -x
    $solana_keygen -o "$SOLANA_CONFIG_DIR"/fullnode-id.json
    $solana_fullnode_config \
      --keypair="$SOLANA_CONFIG_DIR"/fullnode-id.json \
      "$ip_address_arg" -b 9000 > "$SOLANA_CONFIG_DIR"/fullnode.json
  )
fi
