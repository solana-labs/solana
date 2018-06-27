#!/bin/bash

num_tokens=${1:-1000000000}

here=$(dirname "$0")
# shellcheck source=multinode-demo/common.sh
source "$here"/common.sh

set -e

echo "Cleaning $SOLANA_CONFIG_DIR"
rm -rvf "$SOLANA_CONFIG_DIR"
mkdir -p "$SOLANA_CONFIG_DIR"

echo "Creating $SOLANA_CONFIG_DIR/mint.json with $num_tokens tokens"
$solana_mint <<<"$num_tokens" > "$SOLANA_CONFIG_DIR"/mint.json

echo "Creating $SOLANA_CONFIG_DIR/genesis.log"
$solana_genesis < "$SOLANA_CONFIG_DIR"/mint.json > "$SOLANA_CONFIG_DIR"/genesis.log

echo "Creating $SOLANA_CONFIG_DIR/leader.json"
$solana_fullnode_config -d > "$SOLANA_CONFIG_DIR"/leader.json

echo "Creating $SOLANA_CONFIG_DIR/validator.json"
$solana_fullnode_config -d -b 9000 > "$SOLANA_CONFIG_DIR"/validator.json

ls -lh "$SOLANA_CONFIG_DIR/"
