#!/usr/bin/env bash
#
# Start a full node
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
  echo "usage: $0 [-x] [--no-leader-rotation] [rsync network path to bootstrap leader configuration] [network entry point]"
  echo
  echo " Start a full node on the specified network"
  echo
  echo "   -x: runs a new, dynamically-configured full node"
  echo "   --no-leader-rotation: disable leader rotation"
  echo
  exit 1
}

if [[ $1 = -h ]]; then
  usage
fi

if [[ $1 = -x ]]; then
  self_setup=1
  shift
else
  self_setup=0
fi

maybe_no_leader_rotation=
if [[ $1 = --no-leader-rotation ]]; then
  maybe_no_leader_rotation="--no-leader-rotation"
  shift
fi

if [[ -d $SNAP ]]; then
  if [[ $(snapctl get leader-rotation) = false ]]; then
    maybe_no_leader_rotation="--no-leader-rotation"
  fi
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
      if [[ $leader =~ ^[0-9]{1,3}.[0-9]{1,3}.[0-9]{1,3}.[0-9]{1,3}$ ]]; then
        leader_ip=$leader
      else
        leader_ip=$(dig +short "${leader%:*}" | head -n1)

        if [[ -z $leader_ip ]]; then
            usage "Error: unable to resolve IP address for $leader"
        fi
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
  [[ -f $SOLANA_CONFIG_DIR/fullnode.json ]] || {
    echo "$SOLANA_CONFIG_DIR/fullnode.json not found, create it by running:"
    echo
    echo "  ${here}/setup.sh"
    exit 1
  }
  fullnode_id_path=$SOLANA_CONFIG_DIR/fullnode-id.json
  fullnode_json_path=$SOLANA_CONFIG_DIR/fullnode.json
  ledger_config_dir=$SOLANA_CONFIG_DIR/fullnode-ledger
else
  mkdir -p "$SOLANA_CONFIG_DIR"
  fullnode_id_path=$SOLANA_CONFIG_DIR/fullnode-id-x$$.json
  $solana_keygen -o "$fullnode_id_path"

  mkdir -p "$SOLANA_CONFIG_DIR"
  fullnode_json_path=$SOLANA_CONFIG_DIR/fullnode-x$$.json

  # Find an available port in the range 9100-9899
  (( port = 9100 + ($$ % 800) ))
  while true; do
    (( port = port >= 9900 ? 9100 : ++port ))
    if ! nc -z 127.0.0.1 $port; then
      echo "Selected port $port"
      break;
    fi
    echo "Port $port is in use"
  done

  $solana_fullnode_config --keypair="$fullnode_id_path" -l -b "$port" > "$fullnode_json_path"

  ledger_config_dir=$SOLANA_CONFIG_DIR/fullnode-ledger-x$$
fi

[[ -r $fullnode_id_path ]] || {
  echo "$fullnode_id_path does not exist"
  exit 1
}

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

tune_system

set -ex
$rsync -vPr "$rsync_leader_url"/config/ "$ledger_config_dir"
[[ -d $ledger_config_dir/ledger ]] || {
  echo "Unable to retrieve ledger from $rsync_leader_url"
  exit 1
}

$solana_wallet --keypair "$fullnode_id_path" address

# A fullnode requires 3 tokens to function:
# - one token to create an instance of the vote_program with
# - one token for the transaction fee
# - one token to keep the node identity public key valid.
retries=5
while true; do
  if $solana_wallet --keypair "$fullnode_id_path" --network "$leader_address" airdrop 3; then
    break
  fi

  # TODO: Consider moving this retry logic into `solana-wallet airdrop` itself,
  #       currently it does not retry on "Connection refused" errors.
  retries=$((retries - 1))
  if [[ $retries -le 0 ]]; then
    exit 1
  fi
  echo "Airdrop failed. Remaining retries: $retries"
  sleep 1
done

trap 'kill "$pid" && wait "$pid"' INT TERM
$program \
  $maybe_no_leader_rotation \
  --identity "$fullnode_json_path" \
  --network "$leader_address" \
  --ledger "$ledger_config_dir"/ledger \
  > >($fullnode_logger) 2>&1 &
pid=$!
oom_score_adj "$pid" 1000
wait "$pid"
