#!/usr/bin/env bash
#
# Start a fullnode
#
here=$(dirname "$0")
# shellcheck source=multinode-demo/common.sh
source "$here"/common.sh

# shellcheck source=scripts/oom-score-adj.sh
source "$here"/../scripts/oom-score-adj.sh

fullnode_usage() {
  if [[ -n $1 ]]; then
    echo "$*"
    echo
  fi
  cat <<EOF
usage: $0 [--blockstream PATH] [--init-complete-file FILE] [--label LABEL] [--stake LAMPORTS] [--no-voting] [--rpc-port port] [rsync network path to bootstrap leader configuration] [cluster entry point]

Start a full node

  --blockstream PATH        - open blockstream at this unix domain socket location
  --init-complete-file FILE - create this file, if it doesn't already exist, once node initialization is complete
  --label LABEL             - Append the given label to the fullnode configuration files, useful when running
                              multiple fullnodes from the same filesystem location
  --stake LAMPORTS          - Number of lamports to stake
  --no-voting               - start node without vote signer
  --rpc-port port           - custom RPC port for this node

EOF
  exit 1
}

find_entrypoint() {
  declare entrypoint entrypoint_address
  declare shift=0

  if [[ -z $1 ]]; then
    entrypoint=$PWD                   # Default to local tree for rsync
    entrypoint_address=127.0.0.1:8001 # Default to local entrypoint
  elif [[ -z $2 ]]; then
    entrypoint=$1
    entrypoint_address=$entrypoint:8001
    shift=1
  else
    entrypoint=$1
    entrypoint_address=$2
    shift=2
  fi

  echo "$entrypoint" "$entrypoint_address" "$shift"
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

setup_vote_account() {
  declare entrypoint_ip=$1
  declare node_keypair_path=$2
  declare vote_keypair_path=$3
  declare stake=$4

  declare node_keypair
  node_keypair=$($solana_wallet --keypair "$node_keypair_path" address)

  declare vote_keypair
  vote_keypair=$($solana_wallet --keypair "$vote_keypair_path" address)

  if [[ -f "$vote_keypair_path".configured ]]; then
    echo "Vote account has already been configured"
  else
    $solana_wallet --keypair "$node_keypair_path" --url "http://$entrypoint_ip:8899" airdrop "$stake" || return $?

    # Fund the vote account from the node, with the node as the node_keypair
    $solana_wallet --keypair "$node_keypair_path" --url "http://$entrypoint_ip:8899" \
      create-vote-account "$vote_keypair" "$node_keypair" $((stake - 1)) || return $?

    touch "$vote_keypair_path".configured
  fi

  $solana_wallet --keypair "$node_keypair_path" --url "http://$entrypoint_ip:8899" \
    show-vote-account "$vote_keypair"
  return 0
}

ledger_not_setup() {
  echo "Error: $*"
  echo
  echo "Please run: ${here}/setup.sh"
  exit 1
}

args=()
bootstrap_leader=false
stake=42 # number of lamports to assign as stake by default
poll_for_new_genesis_block=0
label=
fullnode_keypair_path=

positional_args=()
while [[ -n $1 ]]; do
  if [[ ${1:0:1} = - ]]; then
    if [[ $1 = --label ]]; then
      label="-$2"
      shift 2
    elif [[ $1 = --bootstrap-leader ]]; then
      bootstrap_leader=true
      shift
    elif [[ $1 = --poll-for-new-genesis-block ]]; then
      poll_for_new_genesis_block=1
      shift
    elif [[ $1 = --blockstream ]]; then
      stake=0
      args+=("$1" "$2")
      shift 2
    elif [[ $1 = --identity ]]; then
      fullnode_keypair_path=$2
      args+=("$1" "$2")
      shift 2
    elif [[ $1 = --enable-rpc-exit ]]; then
      args+=("$1")
      shift
    elif [[ $1 = --init-complete-file ]]; then
      args+=("$1" "$2")
      shift 2
    elif [[ $1 = --stake ]]; then
      stake="$2"
      shift 2
    elif [[ $1 = --no-voting ]]; then
      args+=("$1")
      shift
    elif [[ $1 = --no-sigverify ]]; then
      args+=("$1")
      shift
    elif [[ $1 = --rpc-port ]]; then
      args+=("$1" "$2")
      shift 2
    elif [[ $1 = --dynamic-port-range ]]; then
      args+=("$1" "$2")
      shift 2
    elif [[ $1 = --gossip-port ]]; then
      args+=("$1" "$2")
      shift 2
    elif [[ $1 = -h ]]; then
      fullnode_usage "$@"
    else
      echo "Unknown argument: $1"
      exit 1
    fi
  else
    positional_args+=("$1")
    shift
  fi
done

if $bootstrap_leader; then
  if [[ ${#positional_args[@]} -ne 0 ]]; then
    fullnode_usage "Unknown argument: ${positional_args[0]}"
  fi

  [[ -f "$SOLANA_CONFIG_DIR"/bootstrap-leader-keypair.json ]] ||
    ledger_not_setup "$SOLANA_CONFIG_DIR/bootstrap-leader-keypair.json not found"

  $solana_ledger_tool --ledger "$SOLANA_CONFIG_DIR"/bootstrap-leader-ledger verify

  : "${fullnode_keypair_path:="$SOLANA_CONFIG_DIR"/bootstrap-leader-keypair.json}"
  fullnode_vote_keypair_path="$SOLANA_CONFIG_DIR"/bootstrap-leader-vote-keypair.json
  ledger_config_dir="$SOLANA_CONFIG_DIR"/bootstrap-leader-ledger
  accounts_config_dir="$SOLANA_CONFIG_DIR"/bootstrap-leader-accounts

  default_arg --rpc-port 8899
  default_arg --rpc-drone-address 127.0.0.1:9900
  default_arg --gossip-port 8001
else
  if [[ ${#positional_args[@]} -gt 2 ]]; then
    fullnode_usage "$@"
  fi

  read -r entrypoint entrypoint_address shift < <(find_entrypoint "${positional_args[@]}")
  shift "$shift"

  : "${fullnode_keypair_path:=$SOLANA_CONFIG_DIR/fullnode-id$label.json}"
  fullnode_vote_keypair_path=$SOLANA_CONFIG_DIR/fullnode-vote-id$label.json
  ledger_config_dir=$SOLANA_CONFIG_DIR/fullnode-ledger$label
  accounts_config_dir=$SOLANA_CONFIG_DIR/fullnode-accounts$label

  mkdir -p "$SOLANA_CONFIG_DIR"
  [[ -r "$fullnode_keypair_path" ]] || $solana_keygen -o "$fullnode_keypair_path"
  [[ -r "$fullnode_vote_keypair_path" ]] || $solana_keygen -o "$fullnode_vote_keypair_path"

  default_arg --entrypoint "$entrypoint_address"
  default_arg --rpc-drone-address "${entrypoint_address%:*}:9900"
fi

fullnode_keypair=$($solana_keygen pubkey "$fullnode_keypair_path")
fullnode_vote_keypair=$($solana_keygen pubkey "$fullnode_vote_keypair_path")

cat <<EOF
======================[ Fullnode configuration ]======================
node pubkey: $fullnode_keypair
vote pubkey: $fullnode_vote_keypair
ledger: $ledger_config_dir
accounts: $accounts_config_dir
======================================================================
EOF

if [[ -z $CI ]]; then # Skip in CI
  # shellcheck source=scripts/tune-system.sh
  source "$here"/../scripts/tune-system.sh
fi

default_arg --identity "$fullnode_keypair_path"
default_arg --voting-keypair "$fullnode_vote_keypair_path"
default_arg --vote-account "$fullnode_vote_keypair"
default_arg --ledger "$ledger_config_dir"
default_arg --accounts "$accounts_config_dir"

if [[ -n $SOLANA_CUDA ]]; then
  program=$solana_fullnode_cuda
else
  program=$solana_fullnode
fi

set -e
secs_to_next_genesis_poll=0
PS4="$(basename "$0"): "
while true; do
  if [[ ! -d "$SOLANA_RSYNC_CONFIG_DIR"/ledger ]]; then
    if $bootstrap_leader; then
      ledger_not_setup "$SOLANA_RSYNC_CONFIG_DIR/ledger does not exist"
    fi
    rsync_entrypoint_url=$(rsync_url "$entrypoint")
    $rsync -vPr "$rsync_entrypoint_url"/config/ledger "$SOLANA_RSYNC_CONFIG_DIR"
  fi

  if [[ ! -d "$ledger_config_dir" ]]; then
    cp -a "$SOLANA_RSYNC_CONFIG_DIR"/ledger/ "$ledger_config_dir"
    $solana_ledger_tool --ledger "$ledger_config_dir" verify
  fi

  trap '[[ -n $pid ]] && kill "$pid" >/dev/null 2>&1 && wait "$pid"' INT TERM ERR

  if ! $bootstrap_leader && ((stake)); then
    setup_vote_account "${entrypoint_address%:*}" "$fullnode_keypair_path" "$fullnode_vote_keypair_path" "$stake"
  fi

  echo "$PS4$program ${args[*]}"
  $program "${args[@]}" > >($fullnode_logger) 2>&1 &
  pid=$!
  oom_score_adj "$pid" 1000

  if $bootstrap_leader; then
    wait "$pid"
    sleep 1
  else
    while true; do
      if ! kill -0 "$pid"; then
        wait "$pid"
        exit 0
      fi

      sleep 1

      ((poll_for_new_genesis_block)) || continue
      ((secs_to_next_genesis_poll--)) && continue

      $rsync -r "$rsync_entrypoint_url"/config/ledger "$SOLANA_RSYNC_CONFIG_DIR" || true
      diff -q "$SOLANA_RSYNC_CONFIG_DIR"/ledger/genesis.json "$ledger_config_dir"/genesis.json >/dev/null 2>&1 || break
      secs_to_next_genesis_poll=60

    done

    echo "############## New genesis detected, restarting fullnode ##############"
    kill "$pid" || true
    wait "$pid" || true
    rm -rf "$ledger_config_dir" "$accounts_config_dir" "$fullnode_vote_keypair_path".configured
    sleep 60 # give the network time to come back up
  fi

done
