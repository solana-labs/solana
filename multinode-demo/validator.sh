#!/usr/bin/env bash
#
# Start a validator node
#
here=$(dirname "$0")
# shellcheck source=multinode-demo/common.sh
source "$here"/common.sh

# shellcheck source=scripts/oom-score-adj.sh
source "$here"/../scripts/oom-score-adj.sh

if [[ -d "$SNAP" ]]; then
  # Exit if mode is not yet configured
  # (typically the case after the Snap is first installed)
  [[ -n "$(snapctl get mode)" ]] || exit 0
fi

usage() {
  if [[ -n $1 ]]; then
    echo "$*"
    echo
  fi
  echo "usage: $0 [-x] [rsync network path to leader] [network entry point]"
  echo
  echo " Start a validator on the specified network"
  echo
  echo "   -x: runs a new, dynamically-configured validator"
  echo
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

find_leader() {
  declare leader leader_address
  declare shift=0

  if [[ -d $SNAP ]]; then
    if [[ -n $1 ]]; then
      usage "Error: unexpected parameter: $1"
    fi

    # Select leader from the Snap configuration
    leader_ip=$(snapctl get entrypoint-ip)
    if [[ -z $leader_ip ]]; then
      leader=testnet.solana.com
      leader_ip=$(dig +short "${leader%:*}" | head -n1)
      if [[ -z $leader_ip ]]; then
          usage "Error: unable to resolve IP address for $leader"
      fi
    fi
    leader=$leader_ip
    leader_address=$leader_ip:8001
  else
    if [[ -z $1 ]]; then
      leader=${here}/..        # Default to local tree for rsync
      leader_address=127.0.0.1:8001 # Default to local leader
    elif [[ -z $2 ]]; then
      leader=$1

      declare leader_ip
      leader_ip=$(dig +short "${leader%:*}" | head -n1)

      if [[ -z $leader_ip ]]; then
          usage "Error: unable to resolve IP address for $leader"
      fi

      leader_address=$leader_ip:8001
      shift=1
    else
      leader=$1
      leader_address=$2
      shift=2
    fi
  fi

  echo "$leader" "$leader_address" "$shift"
}

read -r leader leader_address shift < <(find_leader "${@:1:2}")
shift "$shift"

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
  validator_id_path=$SOLANA_CONFIG_PRIVATE_DIR/validator-id.json
  validator_json_path=$SOLANA_CONFIG_VALIDATOR_DIR/validator.json
  SOLANA_LEADER_CONFIG_DIR=$SOLANA_CONFIG_VALIDATOR_DIR/leader-config
else
  mkdir -p "$SOLANA_CONFIG_PRIVATE_DIR"
  validator_id_path=$SOLANA_CONFIG_PRIVATE_DIR/validator-id-x$$.json
  $solana_keygen -o "$validator_id_path"

  mkdir -p "$SOLANA_CONFIG_VALIDATOR_DIR"
  validator_json_path=$SOLANA_CONFIG_VALIDATOR_DIR/validator-x$$.json

  port=9000
  (((port += ($$ % 1000)) && (port == 9000) && port++))

  $solana_fullnode_config --keypair="$validator_id_path" -l -b "$port" > "$validator_json_path"

  SOLANA_LEADER_CONFIG_DIR=$SOLANA_CONFIG_VALIDATOR_DIR/leader-config-x$$
fi

[[ -r $validator_id_path ]] || {
  echo "$validator_id_path does not exist"
  exit 1
}

# A fullnode requires 2 tokens to function:
# - one token to create an instance of the vote_program with
# - one second token to keep the node identity public key valid.
(
  set -x
  $solana_wallet \
    --keypair "$validator_id_path" \
    --network "$leader_address" \
    airdrop 2
)

rsync_url() { # adds the 'rsync://` prefix to URLs that need it
  declare url="$1"

  if [[ $url =~ ^.*:.*$ ]]; then
    # assume remote-shell transport when colon is present, use $url unmodified
    echo "$url"
    return 0
  fi

  if [[ -d $url ]]; then
    # assume local directory if $url is a valid directory, use $url unmodified
    echo "$url"
    return 0
  fi

  # Default to rsync:// URL
  echo "rsync://$url"
}

rsync_leader_url=$(rsync_url "$leader")

tune_networking

set -ex
$rsync -vPr "$rsync_leader_url"/config/ "$SOLANA_LEADER_CONFIG_DIR"
[[ -d $SOLANA_LEADER_CONFIG_DIR/ledger ]] || {
  echo "Unable to retrieve ledger from $rsync_leader_url"
  exit 1
}

trap 'kill "$pid" && wait "$pid"' INT TERM
$program \
  --no-leader-rotation \
  --identity "$validator_json_path" \
  --network "$leader_address" \
  --ledger "$SOLANA_LEADER_CONFIG_DIR"/ledger \
  > >($validator_logger) 2>&1 &
pid=$!
oom_score_adj "$pid" 1000
wait "$pid"
