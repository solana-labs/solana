#!/bin/bash
here=$(dirname "$0")
# shellcheck source=multinode-demo/common.sh
source "$here"/common.sh

usage() {
  if [[ -n $1 ]]; then
    echo "$*"
    echo
  fi
  echo "usage: $0 [-x] [rsync network path to solana repo on leader machine] [network ip address of leader]"
  echo ""
  echo "       -x: runs a new, dynamically-configured validator"
  exit 1
}

if [[ $1 = -h ]]; then
  usage
fi

if [[ $1 == -x ]]; then
  self_setup=1
  shift
else
  self_setup=0
fi

if [[ -n $3 ]]; then
  usage
fi

if [[ -d $SNAP ]]; then
  # Exit if mode is not yet configured
  # (typically the case after the Snap is first installed)
  [[ -n $(snapctl get mode) ]] || exit 0

  # Select leader from the Snap configuration
  leader_address=$(snapctl get leader-address)
  if [[ -z $leader_address ]]; then
    # Assume public testnet by default
    leader_address=35.230.65.68  # testnet.solana.com
  fi
  leader=$leader_address
else
  if [[ -z $1 ]]; then
    leader=${1:-${here}/..}    # Default to local solana repo
    leader_address=${2:-127.0.0.1}  # Default to local leader
  elif [[ -z $2 ]]; then
    leader=$1
    leader_address=$(dig +short "${leader%:*}" | head -n1)
    if [[ -z $leader_address ]]; then
      usage "Error: unable to resolve IP address for $leader"
    fi
  else
    leader=$1
    leader_address=$2
  fi
fi
leader_port=8001

if [[ -n $SOLANA_CUDA ]]; then
  program=$solana_fullnode_cuda
else
  program=$solana_fullnode
fi

if ((!self_setup)); then
  [[ -f $SOLANA_CONFIG_VALIDATOR_DIR/validator.json ]] || {
    echo "$SOLANA_CONFIG_VALIDATOR_DIR/validator.json not found, create it by running:"
    echo
    echo "  ${here}/setup.sh"
    exit 1
  }
  validator_json_path=$SOLANA_CONFIG_VALIDATOR_DIR/validator.json
  SOLANA_LEADER_CONFIG_DIR=$SOLANA_CONFIG_VALIDATOR_DIR/leader-config
else
  mkdir -p "$SOLANA_CONFIG_PRIVATE_DIR"
  validator_id_path=$SOLANA_CONFIG_PRIVATE_DIR/validator-id-x$$.json
  $solana_keygen -o "$validator_id_path"

  mkdir -p "$SOLANA_CONFIG_VALIDATOR_DIR"
  validator_json_path=$SOLANA_CONFIG_VALIDATOR_DIR/validator-x$$.json
  $solana_fullnode_config --keypair="$validator_id_path" -l -b $((9000 + ($$ % 1000))) > "$validator_json_path"

  SOLANA_LEADER_CONFIG_DIR=$SOLANA_CONFIG_VALIDATOR_DIR/leader-config-x$$
fi

rsync_leader_url=$(rsync_url "$leader")

tune_networking

rm -rf "$SOLANA_LEADER_CONFIG_DIR"
set -ex
$rsync -vPrz "$rsync_leader_url"/config/ "$SOLANA_LEADER_CONFIG_DIR"
[[ -r $SOLANA_LEADER_CONFIG_DIR/ledger.log ]] || {
  echo "Unable to retrieve ledger.log from $rsync_leader_url"
  exit 1
}

trap 'kill "$pid" && wait "$pid"' INT TERM
$program \
  --identity "$validator_json_path" \
  --testnet "$leader_address:$leader_port" \
  --ledger "$SOLANA_LEADER_CONFIG_DIR"/ledger.log \
  > >($validator_logger) 2>&1 &
pid=$!
wait "$pid"
