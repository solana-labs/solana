#!/bin/bash

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
                  options are validator or leader.  Creates configuration files
                  for both by default

EOF
  exit $exitcode
}

ip_address_arg=-l
num_tokens=1000000000
node_type_leader=true
node_type_validator=true
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
    leader)
      node_type_leader=true
      node_type_validator=false
      ;;
    validator)
      node_type_leader=false
      node_type_validator=true
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


leader_address_args=("$ip_address_arg")
validator_address_args=("$ip_address_arg" -b 9000)

set -e

echo "Cleaning $SOLANA_CONFIG_DIR"
rm -rvf "$SOLANA_CONFIG_DIR"
mkdir -p "$SOLANA_CONFIG_DIR"


if $node_type_leader; then
  rm -rvf "$SOLANA_CONFIG_PRIVATE_DIR"
  mkdir -p "$SOLANA_CONFIG_PRIVATE_DIR"

  echo "Creating $SOLANA_CONFIG_DIR/mint.json with $num_tokens tokens"
  $solana_keygen -o "$SOLANA_CONFIG_PRIVATE_DIR"/mint.json

  echo "Creating $SOLANA_CONFIG_DIR/genesis.log"
  $solana_genesis --tokens="$num_tokens" < "$SOLANA_CONFIG_PRIVATE_DIR"/mint.json > "$SOLANA_CONFIG_DIR"/genesis.log

  echo "Creating $SOLANA_CONFIG_DIR/leader.json"
  $solana_fullnode_config "${leader_address_args[@]}" > "$SOLANA_CONFIG_DIR"/leader.json
fi


if $node_type_validator; then
  echo "Creating $SOLANA_CONFIG_DIR/validator.json"
  $solana_fullnode_config "${validator_address_args[@]}" > "$SOLANA_CONFIG_DIR"/validator.json
fi

ls -lh "$SOLANA_CONFIG_DIR"/
if $node_type_leader; then
  ls -lh "$SOLANA_CONFIG_PRIVATE_DIR"
fi
