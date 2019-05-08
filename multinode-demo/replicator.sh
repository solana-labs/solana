#!/usr/bin/env bash
#
# Start a relpicator
#
here=$(dirname "$0")
# shellcheck source=multinode-demo/common.sh
source "$here"/common.sh

# shellcheck source=scripts/oom-score-adj.sh
source "$here"/../scripts/oom-score-adj.sh

replicator_usage() {
  if [[ -n $1 ]]; then
    echo "$*"
    echo
  fi
  cat <<EOF
usage: $0 [--stake LAMPORTS] [cluster entry point]

Start a full node

  --blockstream PATH        - open blockstream at this unix domain socket location
  --stake LAMPORTS          - Number of lamports to stake
  --rpc-port port           - custom RPC port for this node

EOF
  exit 1
}

program=$solana_replicator

setup_replicator_account() {
  declare entrypoint_ip=$1
  declare node_id_path=$2
  declare stake=$3

  declare node_id
  node_id=$($solana_wallet --keypair "$node_id_path" address)

  if [[ -f "$node_id_path".configured ]]; then
    echo "Replicator account has already been configured"
  else
    airdrop "$node_id_path" "$entrypoint_ip" "$stake" || return $?
    touch "$node_id_path".configured
  fi

  return 0
}

default_arg() {
  declare name=$1
  declare value=$2

  for arg in "${replicator_args[@]}"; do
    if [[ $arg = "$name" ]]; then
      return
    fi
  done

  if [[ -n $value ]]; then
    replicator_args+=("$name" "$value")
  else
    replicator_args+=("$name")
  fi
}


positional_args=()
replicator_args=()
while [[ -n $1 ]]; do
  if [[ ${1:0:1} = - ]]; then
    if [[ $1 = --stake ]]; then
      stake="$2"
      shift 2
    elif [[ $1 = -h ]]; then
      replicator_usage "$@"
    else
      echo "Unknown argument: $1"
      exit 1
    fi
  else
    positional_args+=("$1")
    shift
  fi
done


if [[ ${#positional_args[@]} -gt 2 ]]; then
  replicator_usage "$@"
fi

read -r leader leader_address shift < <(find_leader "${positional_args[@]}")
shift "$shift"

replicator_id_path=$SOLANA_CONFIG_DIR/replicator-id.json
replicator_storage_id_path="$SOLANA_CONFIG_DIR"/replicator-vote-id.json
ledger_config_dir=$SOLANA_CONFIG_DIR/replicator-ledger

mkdir -p "$SOLANA_CONFIG_DIR"
[[ -r "$replicator_id_path" ]] || $solana_keygen -o "$replicator_id_path"
[[ -r "$replicator_storage_id_path" ]] || $solana_keygen -o "$replicator_storage_id_path"

replicator_id=$($solana_keygen pubkey "$replicator_id_path")
replicator_storage_id=$($solana_keygen pubkey "$replicator_storage_id_path")

default_arg --entrypoint "$leader_address"
default_arg --identity "$replicator_id_path"
default_arg --storage_id "$replicator_storage_id_path"
default_arg --ledger "$ledger_config_dir"

cat <<EOF
======================[ Replicator configuration ]======================
replicator id: $replicator_id
storage id: $replicator_storage_id
ledger: $ledger_config_dir
======================================================================
EOF

if [[ -z $CI ]]; then # Skip in CI
  # shellcheck source=scripts/tune-system.sh
  source "$here"/../scripts/tune-system.sh
fi

if [[ ! -d "$SOLANA_RSYNC_CONFIG_DIR"/ledger ]]; then
  rsync_leader_url=$(rsync_url "$leader")
  $rsync -vPr "$rsync_leader_url"/config/ledger "$SOLANA_RSYNC_CONFIG_DIR"
fi

if [[ ! -d "$ledger_config_dir" ]]; then
  cp -a "$SOLANA_RSYNC_CONFIG_DIR"/ledger/ "$ledger_config_dir"
  $solana_ledger_tool --ledger "$ledger_config_dir" verify
fi

setup_replicator_account ${leader_address%:*} $replicator_id_path 5

$program "${replicator_args[@]}"


