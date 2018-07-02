#!/bin/bash

here=$(dirname "$0")
# shellcheck source=multinode-demo/common.sh
source "$here"/common.sh

usage () {
  cat <<EOF
usage: $0 [-n num_tokens] [-l] [-p]

Creates a fullnode configuration

 -n num_tokens  - Number of tokens to create
 -l             - Detect network address from local machine configuration, which
                  may be a private IP address unaccessible on the Intenet (default)
 -p             - Detect public address using public Internet servers
EOF
}

ip_address_arg=-l
num_tokens=1000000000
while getopts "h?n:lp" opt; do
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
  esac
done


leader_address_args=("$ip_address_arg")
validator_address_args=("$ip_address_arg" -b 9000)

set -e

echo "Cleaning $SOLANA_CONFIG_DIR"
(
  set -x
  rm -rvf "$SOLANA_CONFIG_DIR"{,-private}
  mkdir -p "$SOLANA_CONFIG_DIR"{,-private}
)

echo "Creating $SOLANA_CONFIG_DIR/mint.json with $num_tokens tokens"
$solana_mint <<<"$num_tokens" > "$SOLANA_CONFIG_DIR"-private/mint.json

echo "Creating $SOLANA_CONFIG_DIR/genesis.log"
$solana_genesis < "$SOLANA_CONFIG_DIR"-private/mint.json > "$SOLANA_CONFIG_DIR"/genesis.log

echo "Creating $SOLANA_CONFIG_DIR/leader.json"
$solana_fullnode_config "${leader_address_args[@]}" > "$SOLANA_CONFIG_DIR"/leader.json

echo "Creating $SOLANA_CONFIG_DIR/validator.json"
$solana_fullnode_config "${validator_address_args[@]}" > "$SOLANA_CONFIG_DIR"/validator.json

ls -lh "$SOLANA_CONFIG_DIR/"
