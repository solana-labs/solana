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

Start a validator or a replicator

  --blockstream PATH        - open blockstream at this unix domain socket location
  --init-complete-file FILE - create this file, if it doesn't already exist, once node initialization is complete
  --label LABEL             - Append the given label to the configuration files, useful when running
                              multiple fullnodes in the same workspace
  --stake LAMPORTS          - Number of lamports to stake
  --no-voting               - start node without vote signer
  --rpc-port port           - custom RPC port for this node
  --no-restart              - do not restart the node if it exits
  --no-airdrop              - The genesis block has an account for the node. Airdrops are not required.

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

setup_validator_accounts() {
  declare entrypoint_ip=$1
  declare node_keypair_path=$2
  declare vote_keypair_path=$3
  declare stake_keypair_path=$4
  declare storage_keypair_path=$5
  declare node_lamports=$6
  declare stake_lamports=$7

  declare node_pubkey
  node_pubkey=$($solana_keygen pubkey "$node_keypair_path")

  declare vote_pubkey
  vote_pubkey=$($solana_keygen pubkey "$vote_keypair_path")

  declare stake_pubkey
  stake_pubkey=$($solana_keygen pubkey "$stake_keypair_path")

  declare storage_pubkey
  storage_pubkey=$($solana_keygen pubkey "$storage_keypair_path")

  if [[ -f $configured_flag ]]; then
    echo "Vote and stake accounts have already been configured"
  else
    if ((airdrops_enabled)); then
      # Fund the node with enough tokens to fund its Vote, Staking, and Storage accounts
      declare fees=100 # TODO: No hardcoded transaction fees, fetch the current cluster fees
      $solana_wallet --keypair "$node_keypair_path" --url "http://$entrypoint_ip:8899" airdrop $((node_lamports+stake_lamports+fees)) || return $?
    else
      echo "current account balance is "
      $solana_wallet --keypair "$node_keypair_path" --url "http://$entrypoint_ip:8899" balance || return $?
    fi

    # Fund the vote account from the node, with the node as the node_pubkey
    $solana_wallet --keypair "$node_keypair_path" --url "http://$entrypoint_ip:8899" \
      create-vote-account "$vote_pubkey" "$node_pubkey" 1 --commission 65535 || return $?

    # Fund the stake account from the node, with the node as the node_pubkey
    $solana_wallet --keypair "$node_keypair_path" --url "http://$entrypoint_ip:8899" \
      create-stake-account "$stake_pubkey" "$stake_lamports" || return $?

    # Delegate the stake.  The transaction fee is paid by the node but the
    #  transaction must be signed by the stake_keypair
    $solana_wallet --keypair "$node_keypair_path" --url "http://$entrypoint_ip:8899" \
      delegate-stake "$stake_keypair_path" "$vote_pubkey" "$stake_lamports" || return $?

    # Setup validator storage account
    $solana_wallet --keypair "$node_keypair_path" --url "http://$entrypoint_ip:8899" \
      create-validator-storage-account "$node_pubkey" "$storage_pubkey" || return $?

    touch "$configured_flag"
  fi

  $solana_wallet --keypair "$node_keypair_path" --url "http://$entrypoint_ip:8899" \
    show-vote-account "$vote_pubkey"
  $solana_wallet --keypair "$node_keypair_path" --url "http://$entrypoint_ip:8899" \
    show-stake-account "$stake_pubkey"
  $solana_wallet --keypair "$node_keypair_path" --url "http://$entrypoint_ip:8899" \
    show-storage-account "$storage_pubkey"

  echo "Identity account balance:"
  $solana_wallet --keypair "$identity_keypair_path" --url "http://$entrypoint_ip:8899" balance
  echo "========================================================================"
  return 0
}

setup_replicator_account() {
  declare entrypoint_ip=$1
  declare node_keypair_path=$2
  declare storage_keypair_path=$3
  declare node_lamports=$4

  declare node_pubkey
  node_pubkey=$($solana_keygen pubkey "$node_keypair_path")

  declare storage_pubkey
  storage_pubkey=$($solana_keygen pubkey "$storage_keypair_path")

  if [[ -f $configured_flag ]]; then
    echo "Replicator account has already been configured"
  else
    if ((airdrops_enabled)); then
      $solana_wallet --keypair "$node_keypair_path" --url "http://$entrypoint_ip:8899" airdrop "$node_lamports" || return $?
    else
      echo "current account balance is "
      $solana_wallet --keypair "$node_keypair_path" --url "http://$entrypoint_ip:8899" balance || return $?
    fi

    # Setup replicator storage account
    $solana_wallet --keypair "$node_keypair_path" --url "http://$entrypoint_ip:8899" \
      create-replicator-storage-account "$node_pubkey" "$storage_pubkey" || return $?

    touch "$configured_flag"
  fi

  $solana_wallet --keypair "$node_keypair_path" --url "http://$entrypoint_ip:8899" \
    show-storage-account "$storage_pubkey"

  return 0
}

ledger_not_setup() {
  echo "Error: $*"
  echo
  echo "Please run: ${here}/setup.sh"
  exit 1
}

args=()
node_type=validator
node_lamports=424242  # number of lamports to assign the node for transaction fees
stake_lamports=42     # number of lamports to assign as stake
poll_for_new_genesis_block=0
label=
identity_keypair_path=
no_restart=0
airdrops_enabled=1
generate_snapshots=0

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
      generate_snapshots=1
      shift
    elif [[ $1 = --generate-snapshots ]]; then
      generate_snapshots=1
      shift
    elif [[ $1 = --replicator ]]; then
      node_type=replicator
      shift
    elif [[ $1 = --validator ]]; then
      node_type=validator
      shift
    elif [[ $1 = --poll-for-new-genesis-block ]]; then
      poll_for_new_genesis_block=1
      shift
    elif [[ $1 = --blockstream ]]; then
      stake_lamports=0
      args+=("$1" "$2")
      shift 2
    elif [[ $1 = --identity ]]; then
      identity_keypair_path=$2
      args+=("$1" "$2")
      shift 2
    elif [[ $1 = --enable-rpc-exit ]]; then
      args+=("$1")
      shift
    elif [[ $1 = --init-complete-file ]]; then
      args+=("$1" "$2")
      shift 2
    elif [[ $1 = --stake ]]; then
      stake_lamports="$2"
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
    elif [[ $1 = --no-airdrop ]]; then
      airdrops_enabled=0
      shift
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


if [[ $node_type = replicator ]]; then
  if [[ ${#positional_args[@]} -gt 2 ]]; then
    fullnode_usage "$@"
  fi

  read -r entrypoint entrypoint_address shift < <(find_entrypoint "${positional_args[@]}")
  shift "$shift"

  : "${identity_keypair_path:=$SOLANA_CONFIG_DIR/replicator-keypair$label.json}"
  storage_keypair_path="$SOLANA_CONFIG_DIR"/replicator-storage-keypair$label.json
  ledger_config_dir=$SOLANA_CONFIG_DIR/replicator-ledger$label
  configured_flag=$SOLANA_CONFIG_DIR/replicator$label.configured

  mkdir -p "$SOLANA_CONFIG_DIR"
  [[ -r "$identity_keypair_path" ]] || $solana_keygen new -o "$identity_keypair_path"
  [[ -r "$storage_keypair_path" ]] || $solana_keygen new -o "$storage_keypair_path"

  identity_pubkey=$($solana_keygen pubkey "$identity_keypair_path")
  storage_pubkey=$($solana_keygen pubkey "$storage_keypair_path")

  cat <<EOF
======================[ $node_type configuration ]======================
replicator pubkey: $identity_pubkey
storage pubkey: $storage_pubkey
ledger: $ledger_config_dir
======================================================================
EOF
  program=$solana_replicator
  default_arg --entrypoint "$entrypoint_address"
  default_arg --identity "$identity_keypair_path"
  default_arg --storage-keypair "$storage_keypair_path"
  default_arg --ledger "$ledger_config_dir"

  rsync_entrypoint_url=$(rsync_url "$entrypoint")
elif [[ $node_type = bootstrap_leader ]]; then
  if [[ ${#positional_args[@]} -ne 0 ]]; then
    fullnode_usage "Unknown argument: ${positional_args[0]}"
  fi

  [[ -f "$SOLANA_CONFIG_DIR"/bootstrap-leader-keypair.json ]] ||
    ledger_not_setup "$SOLANA_CONFIG_DIR/bootstrap-leader-keypair.json not found"

  $solana_ledger_tool --ledger "$SOLANA_CONFIG_DIR"/bootstrap-leader-ledger verify

  : "${identity_keypair_path:=$SOLANA_CONFIG_DIR/bootstrap-leader-keypair.json}"
  vote_keypair_path="$SOLANA_CONFIG_DIR"/bootstrap-leader-vote-keypair.json
  ledger_config_dir="$SOLANA_CONFIG_DIR"/bootstrap-leader-ledger
  state_dir="$SOLANA_CONFIG_DIR"/bootstrap-leader-state
  storage_keypair_path=$SOLANA_CONFIG_DIR/bootstrap-leader-storage-keypair.json
  configured_flag=$SOLANA_CONFIG_DIR/bootstrap-leader.configured

  default_arg --rpc-port 8899
  if ((airdrops_enabled)); then
    default_arg --rpc-drone-address 127.0.0.1:9900
  fi
  default_arg --gossip-port 8001

elif [[ $node_type = validator ]]; then
  if [[ ${#positional_args[@]} -gt 2 ]]; then
    fullnode_usage "$@"
  fi

  read -r entrypoint entrypoint_address shift < <(find_entrypoint "${positional_args[@]}")
  shift "$shift"

  : "${identity_keypair_path:=$SOLANA_CONFIG_DIR/validator-keypair$label.json}"
  vote_keypair_path=$SOLANA_CONFIG_DIR/validator-vote-keypair$label.json
  ledger_config_dir=$SOLANA_CONFIG_DIR/validator-ledger$label
  state_dir="$SOLANA_CONFIG_DIR"/validator-state$label
  storage_keypair_path=$SOLANA_CONFIG_DIR/validator-storage-keypair$label.json
  stake_keypair_path=$SOLANA_CONFIG_DIR/validator-stake-keypair$label.json
  configured_flag=$SOLANA_CONFIG_DIR/validator$label.configured

  mkdir -p "$SOLANA_CONFIG_DIR"
  [[ -r "$identity_keypair_path" ]] || $solana_keygen new -o "$identity_keypair_path"
  [[ -r "$vote_keypair_path" ]] || $solana_keygen new -o "$vote_keypair_path"
  [[ -r "$stake_keypair_path" ]] || $solana_keygen new -o "$stake_keypair_path"
  [[ -r "$storage_keypair_path" ]] || $solana_keygen new -o "$storage_keypair_path"

  default_arg --entrypoint "$entrypoint_address"
  if ((airdrops_enabled)); then
    default_arg --rpc-drone-address "${entrypoint_address%:*}:9900"
  fi

  rsync_entrypoint_url=$(rsync_url "$entrypoint")
else
  echo "Error: Unknown node_type: $node_type"
  exit 1
fi


if [[ $node_type != replicator ]]; then
  accounts_config_dir="$state_dir"/accounts
  snapshot_config_dir="$state_dir"/snapshots

  identity_pubkey=$($solana_keygen pubkey "$identity_keypair_path")
  vote_pubkey=$($solana_keygen pubkey "$vote_keypair_path")
  storage_pubkey=$($solana_keygen pubkey "$storage_keypair_path")

  cat <<EOF
======================[ $node_type configuration ]======================
identity pubkey: $identity_pubkey
vote pubkey: $vote_pubkey
storage pubkey: $storage_pubkey
ledger: $ledger_config_dir
accounts: $accounts_config_dir
snapshots: $snapshot_config_dir
========================================================================
EOF

  default_arg --identity "$identity_keypair_path"
  default_arg --voting-keypair "$vote_keypair_path"
  default_arg --vote-account "$vote_pubkey"
  default_arg --storage-keypair "$storage_keypair_path"
  default_arg --ledger "$ledger_config_dir"
  default_arg --accounts "$accounts_config_dir"
  default_arg --snapshot-path "$snapshot_config_dir"

  if [[ -n $SOLANA_CUDA ]]; then
    program=$solana_validator_cuda
  else
    program=$solana_validator
  fi
fi

if [[ -z $CI ]]; then # Skip in CI
  # shellcheck source=scripts/tune-system.sh
  source "$here"/../scripts/tune-system.sh
fi

new_gensis_block() {
  ! diff -q "$SOLANA_RSYNC_CONFIG_DIR"/ledger/genesis.bin "$ledger_config_dir"/genesis.bin >/dev/null 2>&1
}

set -e
PS4="$(basename "$0"): "
pid=
trap '[[ -n $pid ]] && kill "$pid" >/dev/null 2>&1 && wait "$pid"' INT TERM ERR
while true; do
  if new_gensis_block; then
    # If the genesis block has changed remove the now stale ledger and vote
    # keypair for the node and start all over again
    (
      set -x
      rm -rf "$ledger_config_dir" "$state_dir" "$configured_flag"
    )
  fi

  if [[ ! -d "$SOLANA_RSYNC_CONFIG_DIR"/ledger ]]; then
    if [[ $node_type = bootstrap_leader ]]; then
      ledger_not_setup "$SOLANA_RSYNC_CONFIG_DIR/ledger does not exist"
    elif [[ $node_type = validator ]]; then
      (
        SECONDS=0
        set -x
        cd "$SOLANA_RSYNC_CONFIG_DIR"
        $rsync -qPr "${rsync_entrypoint_url:?}"/config/{ledger,state.tgz} .
        echo "Fetched snapshot in $SECONDS seconds"
      ) || true
    fi
  fi

  (
    set -x
    if [[ $node_type = validator ]]; then
        if [[ -f "$SOLANA_RSYNC_CONFIG_DIR"/state.tgz ]]; then
          mkdir -p "$state_dir"
          SECONDS=
          tar -C "$state_dir" -zxf "$SOLANA_RSYNC_CONFIG_DIR"/state.tgz
          echo "Extracted snapshot in $SECONDS seconds"
        fi
    fi
    if [[ ! -d "$ledger_config_dir" ]]; then
      cp -a "$SOLANA_RSYNC_CONFIG_DIR"/ledger/ "$ledger_config_dir"
    fi
  )

  if ((stake_lamports)); then
    if [[ $node_type = validator ]]; then
      setup_validator_accounts "${entrypoint_address%:*}" \
        "$identity_keypair_path" \
        "$vote_keypair_path" \
        "$stake_keypair_path" \
        "$storage_keypair_path" \
        "$node_lamports" \
        "$stake_lamports"
    elif [[ $node_type = replicator ]]; then
      setup_replicator_account "${entrypoint_address%:*}" \
        "$identity_keypair_path" \
        "$storage_keypair_path" \
        "$node_lamports"
    fi
  fi

  echo "$PS4$program ${args[*]}"
  $program "${args[@]}" &
  pid=$!
  oom_score_adj "$pid" 1000

  if ((no_restart)); then
    wait "$pid"
    exit $?
  fi

  secs_to_next_genesis_poll=5
  secs_to_next_snapshot=30
  while true; do
    if ! kill -0 "$pid"; then
      wait "$pid" || true
      echo "############## $node_type exited, restarting ##############"
      break
    fi

    sleep 1

    if ((generate_snapshots && --secs_to_next_snapshot == 0)); then
      (
        SECONDS=
        new_state_dir="$SOLANA_RSYNC_CONFIG_DIR"/new_state
        new_state_archive="$SOLANA_RSYNC_CONFIG_DIR"/new_state.tgz
        (
          rm -rf "$new_state_dir" "$new_state_archive"
          cp -a "$state_dir" "$new_state_dir"
          cd "$new_state_dir"
          tar zcfS "$new_state_archive" ./*
        )
        ln -f "$new_state_archive" "$SOLANA_RSYNC_CONFIG_DIR"/state.tgz
        rm -rf "$new_state_dir" "$new_state_archive"
        ls -hl "$SOLANA_RSYNC_CONFIG_DIR"/state.tgz
        echo "Snapshot generated in $SECONDS seconds"
      ) || (
        echo "Error: failed to generate snapshot"
      )
      secs_to_next_snapshot=60
    fi

    if ((poll_for_new_genesis_block && --secs_to_next_genesis_poll == 0)); then
      echo "Polling for new genesis block..."
      (
        set -x
        $rsync -r "${rsync_entrypoint_url:?}"/config/ledger "$SOLANA_RSYNC_CONFIG_DIR"
      ) || (
        echo "Error: failed to rsync ledger"
      )
      new_gensis_block && break
      secs_to_next_genesis_poll=60
    fi

  done

  echo "############## New genesis detected, restarting $node_type ##############"
  kill "$pid" || true
  wait "$pid" || true
  # give the cluster time to come back up
  (
    set -x
    sleep 60
  )
done
