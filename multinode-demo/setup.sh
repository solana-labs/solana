#!/bin/bash

num_tokens=1000000000
public_ip=

here=$(dirname "$0")
# shellcheck source=multinode-demo/common.sh
source "$here"/common.sh

usage () {
  cat <<EOF
usage: $0 [-n num_tokens] [-P] [-p public_ip_address]

Creates a fullnode configuration

 -n num_tokens         - Number of tokens to create
 -p public_ip_address  - Public IP address to advertise
                         (default uses the system IP address, which may be
                          on a private network)
 -P                    - Autodetect the public IP address of the machine
EOF
}

while getopts "h?n:p:P" opt; do
  case $opt in
  h|\?)
    usage
    exit 0
    ;;
  p)
    public_ip="$OPTARG"
    ;;
  n)
    num_tokens="$OPTARG"
    ;;
  P)
    public_ip="$(curl -s ifconfig.co)"
    echo "Public IP autodetected as $public_ip"
    ;;
  esac
done


if [[ -n "$public_ip" ]]; then
  leader_address_args=(-b "$public_ip":8000)
  validator_address_args=(-b "$public_ip":9000)
else
  leader_address_args=(-d)
  validator_address_args=(-d -b 9000)
fi

set -e

echo "Cleaning $SOLANA_CONFIG_DIR"
rm -rvf "$SOLANA_CONFIG_DIR"
mkdir -p "$SOLANA_CONFIG_DIR"

echo "Creating $SOLANA_CONFIG_DIR/mint.json with $num_tokens tokens"
$solana_mint <<<"$num_tokens" > "$SOLANA_CONFIG_DIR"/mint.json

echo "Creating $SOLANA_CONFIG_DIR/genesis.log"
$solana_genesis < "$SOLANA_CONFIG_DIR"/mint.json > "$SOLANA_CONFIG_DIR"/genesis.log

echo "Creating $SOLANA_CONFIG_DIR/leader.json"
$solana_fullnode_config "${leader_address_args[@]}" > "$SOLANA_CONFIG_DIR"/leader.json

echo "Creating $SOLANA_CONFIG_DIR/validator.json"
$solana_fullnode_config "${validator_address_args[@]}" > "$SOLANA_CONFIG_DIR"/validator.json

ls -lh "$SOLANA_CONFIG_DIR/"
