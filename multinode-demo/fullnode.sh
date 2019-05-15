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

Fullnode Usage:
usage: $0 [--blockstream PATH] [--init-complete-file FILE] [--label LABEL] [--stake LAMPORTS] [--no-voting] [--rpc-port port] [rsync network path to bootstrap leader configuration] [cluster entry point]

Start a full node or a replicator

  --blockstream PATH        - open blockstream at this unix domain socket location
  --init-complete-file FILE - create this file, if it doesn't already exist, once node initialization is complete
  --label LABEL             - Append the given label to the fullnode configuration files, useful when running
                              multiple fullnodes from the same filesystem location
  --stake LAMPORTS          - Number of lamports to stake
  --no-voting               - start node without vote signer
  --rpc-port port           - custom RPC port for this node
  --no-restart              - do not restart the node if it exits

EOF
  exit 1
}

find_entrypoint() {
  declare entrypoint entrypoint_address
  declare shift=0

  if [[ -z $1 ]]; then
    entrypoint="$SOLANA_ROOT"         # Default to local tree for rsync
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

setup_vote_and_stake_accounts() {
  declare entrypoint_ip=$1
  declare node_keypair_path=$2
  declare vote_keypair_path=$3
  declare stake_keypair_path=$4
  declare stake=$5

  declare node_pubkey
  node_pubkey=$($solana_keygen pubkey "$node_keypair_path")

  declare vote_pubkey
  vote_pubkey=$($solana_keygen pubkey "$vote_keypair_path")

  declare stake_pubkey
  stake_pubkey=$($solana_keygen pubkey "$stake_keypair_path")

  if [[ -f "$node_keypair_path".configured ]]; then
    echo "Vote and stake accounts have already been configured"
  else
    $solana_wallet --keypair "$node_keypair_path" --url "http://$entrypoint_ip:8899" airdrop $((stake*2+1)) || return $?

    # Fund the vote account from the node, with the node as the node_pubkey
    $solana_wallet --keypair "$node_keypair_path" --url "http://$entrypoint_ip:8899" \
          create-vote-account "$vote_pubkey" "$node_pubkey" "$stake" || return $?

    # Fund the stake account from the node, with the node as the node_pubkey
    $solana_wallet --keypair "$node_keypair_path" --url "http://$entrypoint_ip:8899" \
           create-stake-account "$stake_pubkey" "$stake" || return $?

    # Delegate the stake.  The transaction fee is paid by the node but the
    #  transaction must be signed by the stake_keypair
    $solana_wallet --keypair "$node_keypair_path" --url "http://$entrypoint_ip:8899" \
           delegate-stake "$stake_keypair_path" "$vote_pubkey" || return $?


    touch "$node_keypair_path".configured
  fi

  $solana_wallet --keypair "$node_keypair_path" --url "http://$entrypoint_ip:8899" \
          show-vote-account "$vote_pubkey"

  $solana_wallet --keypair "$node_keypair_path" --url "http://$entrypoint_ip:8899" \
          show-stake-account "$stake_pubkey"


  return 0
}

ledger_not_setup() {
  echo "Error: $*"
  echo
  echo "Please run: ${here}/setup.sh"
  exit 1
}

setup_replicator_account() {
  declare entrypoint_ip=$1
  declare node_keypair_path=$2
  declare stake=$3

  if [[ -f "$node_keypair_path".configured ]]; then
    echo "Replicator account has already been configured"
  else
    $solana_wallet --keypair "$node_keypair_path" --url "http://$entrypoint_ip:8899" airdrop "$stake" || return $?
    touch "$node_keypair_path".configured
  fi
}

args=()
node_type=validator
stake=42 # number of lamports to assign as stake
poll_for_new_genesis_block=0
label=
fullnode_keypair_path=
no_restart=0

positional_args=()
while [[ -n $1 ]]; do
  if [[ ${1:0:1} = - ]]; then
    if [[ $1 = --label ]]; then
      label="-$2"
      shift 2
    elif [[ $1 = --no-restart ]]; then
      no_restart=1
      shift
    elif [[ $1 = --bootstrap-leader ]]; then
      node_type=bootstrap_leader
      shift
    elif [[ $1 = --replicator ]]; then
      node_type=replicator
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

if [[ $node_type = bootstrap_leader ]]; then
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
  fullnode_storage_keypair_path=$SOLANA_CONFIG_DIR/bootstrap-leader-storage-keypair.json


  default_arg --rpc-port 8899
  default_arg --rpc-drone-address 127.0.0.1:9900
  default_arg --gossip-port 8001

elif [[ $node_type = replicator ]]; then

  if [[ ${#positional_args[@]} -gt 2 ]]; then
    fullnode_usage "Unknown arguments for replicator"
  fi

  read -r entrypoint entrypoint_address shift < <(find_entrypoint "${positional_args[@]}")
  shift "$shift"

  replicator_keypair_path=$SOLANA_CONFIG_DIR/replicator-id.json
  replicator_storage_keypair_path="$SOLANA_CONFIG_DIR"/replicator-storage-id.json
  ledger_config_dir=$SOLANA_CONFIG_DIR/replicator-ledger

  mkdir -p "$SOLANA_CONFIG_DIR"
  [[ -r "$replicator_keypair_path" ]] || $solana_keygen -o "$replicator_keypair_path"
  [[ -r "$replicator_storage_keypair_path" ]] || $solana_keygen -o "$replicator_storage_keypair_path"

  replicator_pubkey=$($solana_keygen pubkey "$replicator_keypair_path")
  replicator_storage_pubkey=$($solana_keygen pubkey "$replicator_storage_keypair_path")

  default_arg --entrypoint "$entrypoint_address"
  default_arg --identity "$replicator_keypair_path"
  default_arg --storage-keypair "$replicator_storage_keypair_path"
  default_arg --ledger "$ledger_config_dir"

else
  if [[ ${#positional_args[@]} -gt 2 ]]; then
    fullnode_usage "$@"
  fi

  read -r entrypoint entrypoint_address shift < <(find_entrypoint "${positional_args[@]}")
  shift "$shift"

  : "${fullnode_keypair_path:=$SOLANA_CONFIG_DIR/fullnode-keypair$label.json}"
  fullnode_vote_keypair_path=$SOLANA_CONFIG_DIR/fullnode-vote-keypair$label.json
  fullnode_stake_keypair_path=$SOLANA_CONFIG_DIR/fullnode-stake-keypair$label.json
  fullnode_storage_keypair_path=$SOLANA_CONFIG_DIR/fullnode-storage-keypair$label.json
  ledger_config_dir=$SOLANA_CONFIG_DIR/fullnode-ledger$label
  accounts_config_dir=$SOLANA_CONFIG_DIR/fullnode-accounts$label

  mkdir -p "$SOLANA_CONFIG_DIR"
  [[ -r "$fullnode_keypair_path" ]] || $solana_keygen -o "$fullnode_keypair_path"
  [[ -r "$fullnode_vote_keypair_path" ]] || $solana_keygen -o "$fullnode_vote_keypair_path"
  [[ -r "$fullnode_stake_keypair_path" ]] || $solana_keygen -o "$fullnode_stake_keypair_path"
  [[ -r "$fullnode_storage_keypair_path" ]] || $solana_keygen -o "$fullnode_storage_keypair_path"

  default_arg --entrypoint "$entrypoint_address"
  default_arg --rpc-drone-address "${entrypoint_address%:*}:9900"

  rsync_entrypoint_url=$(rsync_url "$entrypoint")
fi


if [[ $node_type = replicator ]]; then
  cat <<EOF
======================[ Replicator configuration ]======================
replicator pubkey: $replicator_pubkey
storage pubkey: $replicator_storage_pubkey
ledger: $ledger_config_dir
======================================================================
EOF
  program=$solana_replicator

else

  fullnode_pubkey=$($solana_keygen pubkey "$fullnode_keypair_path")
  fullnode_vote_pubkey=$($solana_keygen pubkey "$fullnode_vote_keypair_path")

  cat <<EOF
======================[ Fullnode configuration ]======================
node pubkey: $fullnode_pubkey
vote pubkey: $fullnode_vote_pubkey
ledger: $ledger_config_dir
accounts: $accounts_config_dir
======================================================================
EOF

  default_arg --identity "$fullnode_keypair_path"
  default_arg --voting-keypair "$fullnode_vote_keypair_path"
  default_arg --vote-account "$fullnode_vote_pubkey"
  default_arg --storage-keypair "$fullnode_storage_keypair_path"
  default_arg --ledger "$ledger_config_dir"
  default_arg --accounts "$accounts_config_dir"

  if [[ -n $SOLANA_CUDA ]]; then
    program=$solana_fullnode_cuda
  else
    program=$solana_fullnode
  fi

fi

if [[ -z $CI ]]; then # Skip in CI
  # shellcheck source=scripts/tune-system.sh
  source "$here"/../scripts/tune-system.sh
fi

new_gensis_block() {
  ! diff -q "$SOLANA_RSYNC_CONFIG_DIR"/ledger/genesis.json "$ledger_config_dir"/genesis.json >/dev/null 2>&1
}

set -e
PS4="$(basename "$0"): "
while true; do
  if [[ ! -d "$SOLANA_RSYNC_CONFIG_DIR"/ledger ]]; then
    if [[ $node_type = bootstrap_leader ]]; then
      ledger_not_setup "$SOLANA_RSYNC_CONFIG_DIR/ledger does not exist"
    fi
    $rsync -vPr "${rsync_entrypoint_url:?}"/config/ledger "$SOLANA_RSYNC_CONFIG_DIR"
  fi

  if new_gensis_block; then
    # If the genesis block has changed remove the now stale ledger and vote
    # keypair for the node and start all over again
    (
      set -x
      rm -rf "$ledger_config_dir" "$accounts_config_dir" \
        "$fullnode_vote_keypair_path".configured "$replicator_storage_keypair_path".configured
    )
  fi

  if [[ ! -d "$ledger_config_dir" ]]; then
    cp -a "$SOLANA_RSYNC_CONFIG_DIR"/ledger/ "$ledger_config_dir"
    $solana_ledger_tool --ledger "$ledger_config_dir" verify
  fi

  trap '[[ -n $pid ]] && kill "$pid" >/dev/null 2>&1 && wait "$pid"' INT TERM ERR

  if [[ $node_type = validator ]] && ((stake)); then
    setup_vote_and_stake_accounts "${entrypoint_address%:*}" "$fullnode_keypair_path" "$fullnode_vote_keypair_path" "$fullnode_stake_keypair_path" "$stake"
  elif [[ $node_type = replicator ]] && ((stake)); then
    setup_replicator_account "${entrypoint_address%:*}" "$replicator_keypair_path" "$stake"
  fi

  echo "$PS4$program ${args[*]}"
  $program "${args[@]}" &
  pid=$!
  oom_score_adj "$pid" 1000

  if ((no_restart)); then
    wait "$pid"
    exit $?
  fi

  if [[ $node_type = bootstrap_leader ]]; then
    wait "$pid"
    sleep 1
  else
    secs_to_next_genesis_poll=1
    while true; do
      if ! kill -0 "$pid"; then
        wait "$pid"
        exit 0
      fi

      sleep 1

      ((poll_for_new_genesis_block)) || continue
      ((secs_to_next_genesis_poll--)) && continue
      (
        set -x
        $rsync -r "${rsync_entrypoint_url:?}"/config/ledger "$SOLANA_RSYNC_CONFIG_DIR"
      ) || true
      new_gensis_block && break

      secs_to_next_genesis_poll=60
    done

    echo "############## New genesis detected, restarting $node_type ##############"
    kill "$pid" || true
    wait "$pid" || true
    # give the cluster time to come back up
    (
      set -x
      sleep 60
    )
  fi
done
