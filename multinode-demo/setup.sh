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
leader_id_path="$SOLANA_CONFIG_PRIVATE_DIR"/leader-id.json
validator_id_path="$SOLANA_CONFIG_PRIVATE_DIR"/validator-id.json
mint_path="$SOLANA_CONFIG_PRIVATE_DIR"/mint.json

set -e

for i in "$SOLANA_CONFIG_DIR" "$SOLANA_CONFIG_PRIVATE_DIR" "$SOLANA_CONFIG_VALIDATOR_DIR"; do
  echo "Cleaning $i"
  rm -rvf "$i"
  mkdir -p "$i"
done


$solana_keygen -o "$leader_id_path"
$solana_keygen -o "$validator_id_path"

if $node_type_leader; then
  echo "Creating $mint_path with $num_tokens tokens"
  $solana_keygen -o "$mint_path"

  echo "Creating $SOLANA_CONFIG_DIR/ledger"
  $solana_genesis --tokens="$num_tokens" --ledger "$SOLANA_CONFIG_DIR"/ledger < "$mint_path"

  echo "Creating $SOLANA_CONFIG_DIR/leader.json"
  $solana_fullnode_config --keypair="$leader_id_path" "${leader_address_args[@]}" > "$SOLANA_CONFIG_DIR"/leader.json
fi


if $node_type_validator; then
  echo "Creating $SOLANA_CONFIG_VALIDATOR_DIR/validator.json"
  $solana_fullnode_config --keypair="$validator_id_path" "${validator_address_args[@]}" > "$SOLANA_CONFIG_VALIDATOR_DIR"/validator.json
fi

ls -lhR "$SOLANA_CONFIG_DIR"/
if $node_type_leader; then
  ls -lhR "$SOLANA_CONFIG_PRIVATE_DIR"
fi
