#!/usr/bin/env bash
#
# Start a validator
#
here=$(dirname "$0")
# shellcheck source=multinode-demo/common.sh
source "$here"/common.sh

usage() {
  if [[ -n $1 ]]; then
    echo "$*"
    echo
  fi
  cat <<EOF

usage: $0 [OPTIONS] [cluster entry point hostname]

Start a validator with no stake

OPTIONS:
  --ledger PATH             - store ledger under this PATH
  --blockstream PATH        - open blockstream at this unix domain socket location
  --init-complete-file FILE - create this file, if it doesn't already exist, once node initialization is complete
  --label LABEL             - Append the given label to the configuration files, useful when running
                              multiple validators in the same workspace
  --node-lamports LAMPORTS  - Number of lamports this node has been funded from the genesis block
  --no-voting               - start node without vote signer
  --rpc-port port           - custom RPC port for this node
  --no-restart              - do not restart the node if it exits
  --no-airdrop              - The genesis block has an account for the node. Airdrops are not required.

EOF
  exit 1
}

args=()
airdrops_enabled=1
node_lamports=424242  # number of lamports to airdrop the node for transaction fees (ignored if airdrops_enabled=0)
poll_for_new_genesis_block=0
label=
identity_keypair_path=
voting_keypair_path=
no_restart=0
# TODO: Enable boot_from_snapshot when snapshots work
#boot_from_snapshot=1
boot_from_snapshot=0
reset_ledger=0
gossip_entrypoint=
ledger_dir=

positional_args=()
while [[ -n $1 ]]; do
  if [[ ${1:0:1} = - ]]; then
    if [[ $1 = --label ]]; then
      label="-$2"
      shift 2
    elif [[ $1 = --no-restart ]]; then
      no_restart=1
      shift
    elif [[ $1 = --no-snapshot ]]; then
      boot_from_snapshot=0
      shift
    elif [[ $1 = --poll-for-new-genesis-block ]]; then
      poll_for_new_genesis_block=1
      shift
    elif [[ $1 = --blockstream ]]; then
      args+=("$1" "$2")
      shift 2
    elif [[ $1 = --entrypoint ]]; then
      gossip_entrypoint=$2
      args+=("$1" "$2")
      shift 2
    elif [[ $1 = --identity ]]; then
      identity_keypair_path=$2
      args+=("$1" "$2")
      shift 2
    elif [[ $1 = --voting-keypair ]]; then
      voting_keypair_path=$2
      args+=("$1" "$2")
      shift 2
    elif [[ $1 = --storage-keypair ]]; then
      storage_keypair_path=$2
      args+=("$1" "$2")
      shift 2
    elif [[ $1 = --enable-rpc-exit ]]; then
      args+=("$1")
      shift
    elif [[ $1 = --init-complete-file ]]; then
      args+=("$1" "$2")
      shift 2
    elif [[ $1 = --node-lamports ]]; then
      node_lamports="$2"
      shift 2
    elif [[ $1 = --no-voting ]]; then
      args+=("$1")
      shift
    elif [[ $1 = --skip-ledger-verify ]]; then
      args+=("$1")
      shift
    elif [[ $1 = --dev-no-sigverify ]]; then
      args+=("$1")
      shift
    elif [[ $1 = --limit-ledger-size ]]; then
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
    elif [[ $1 = --reset-ledger ]]; then
      reset_ledger=1
      shift
    elif [[ $1 = --ledger ]]; then
      ledger_dir=$2
      shift 2
    elif [[ $1 = -h ]]; then
      usage "$@"
    else
      echo "Unknown argument: $1"
      exit 1
    fi
  else
    positional_args+=("$1")
    shift
  fi
done

if [[ ${#positional_args[@]} -gt 1 ]]; then
  usage "$@"
fi

if [[ -n $REQUIRE_LEDGER_DIR ]]; then
  if [[ -z $ledger_dir ]]; then
    usage "Error: --ledger not specified"
  fi
  SOLANA_CONFIG_DIR="$ledger_dir"
fi

if [[ -n $REQUIRE_KEYPAIRS ]]; then
  if [[ -z $identity_keypair_path ]]; then
    usage "Error: --identity not specified"
  fi
  if [[ -z $voting_keypair_path ]]; then
    usage "Error: --voting-keypair not specified"
  fi
fi

if [[ -z "$ledger_dir" ]]; then
  ledger_dir="$SOLANA_CONFIG_DIR/validator$label"
fi
mkdir -p "$ledger_dir"

setup_secondary_mount

if [[ -n $gossip_entrypoint ]]; then
  # Prefer the --entrypoint argument if supplied...
  if [[ ${#positional_args[@]} -gt 0 ]]; then
    usage "$@"
  fi
else
  # ...but also support providing the entrypoint's hostname as the first
  #    positional argument
  entrypoint_hostname=${positional_args[0]}
  if [[ -z $entrypoint_hostname ]]; then
    gossip_entrypoint=127.0.0.1:8001
  else
    gossip_entrypoint="$entrypoint_hostname":8001
  fi
fi
rpc_url=$("$here"/rpc-url.sh "$gossip_entrypoint")
drone_address="${gossip_entrypoint%:*}":9900

: "${identity_keypair_path:=$ledger_dir/identity-keypair.json}"
: "${voting_keypair_path:=$ledger_dir/vote-keypair.json}"
: "${storage_keypair_path:=$ledger_dir/storage-keypair.json}"

default_arg --entrypoint "$gossip_entrypoint"
if ((airdrops_enabled)); then
  default_arg --rpc-drone-address "$drone_address"
fi

accounts_dir="$ledger_dir"/accounts
snapshot_dir="$ledger_dir"/snapshots

default_arg --identity "$identity_keypair_path"
default_arg --voting-keypair "$voting_keypair_path"
default_arg --storage-keypair "$storage_keypair_path"
default_arg --ledger "$ledger_dir"
default_arg --accounts "$accounts_dir"
#default_arg --snapshot-interval-slots 100

if [[ -n $SOLANA_CUDA ]]; then
  program=$solana_validator_cuda
else
  program=$solana_validator
fi

if [[ -z $CI ]]; then # Skip in CI
  # shellcheck source=scripts/tune-system.sh
  source "$here"/../scripts/tune-system.sh
fi

new_genesis_block() {
  if [[ ! -d "$ledger_dir" ]]; then
    return
  fi

  rm -f "$ledger_dir"/new-genesis.tgz
  (
    set -x
    curl -f "$rpc_url"/genesis.tgz -o "$ledger_dir"/new-genesis.tgz
  ) || {
    echo "Error: failed to fetch new genesis ledger"
  }
  ! diff -q "$ledger_dir"/new-genesis.tgz "$ledger_dir"/genesis.tgz >/dev/null 2>&1
}

set -e
PS4="$(basename "$0"): "

pid=
kill_node() {
  # Note: do not echo anything from this function to ensure $pid is actually
  # killed when stdout/stderr are redirected
  set +ex
  if [[ -n $pid ]]; then
    declare _pid=$pid
    pid=
    kill "$_pid" || true
    wait "$_pid" || true
  fi
  exit
}
kill_node_and_exit() {
  kill_node
  exit
}
trap 'kill_node_and_exit' INT TERM ERR

if ((reset_ledger)); then
  echo "Resetting ledger..."
  (
    set -x
    rm -rf "$ledger_dir"
  )
fi


wallet() {
  (
    set -x
    $solana_wallet --keypair "$identity_keypair_path" --url "$rpc_url" "$@"
  )
}

setup_validator_accounts() {
  declare node_lamports=$1

  if ((airdrops_enabled)); then
    echo "Adding $node_lamports to validator identity account:"
    (
      declare fees=100 # TODO: No hardcoded transaction fees, fetch the current cluster fees
      wallet airdrop $((node_lamports+fees))
    ) || return $?
  else
    echo "Validator identity account balance:"
    wallet balance || return $?
  fi

  if ! wallet show-vote-account "$vote_pubkey"; then
    echo "Creating validator vote account"
    wallet create-vote-account "$vote_pubkey" "$identity_pubkey" 1 --commission 127 || return $?
  fi
  echo "Validator vote account configured"

  if ! wallet show-storage-account "$storage_pubkey"; then
    echo "Creating validator storage account"
    wallet create-validator-storage-account "$identity_pubkey" "$storage_pubkey" || return $?
  fi
  echo "Validator storage account configured"

  return 0
}

while true; do
  if new_genesis_block; then
    # If the genesis block has changed remove the now stale ledger and start all
    # over again
    (
      set -x
      rm -rf "$ledger_dir"
    )
  fi

  if [[ ! -f "$ledger_dir"/.genesis ]]; then
      echo "Fetching ledger from $rpc_url/genesis.tgz..."
      SECONDS=
      mkdir -p "$ledger_dir"
      while ! curl -f "$rpc_url"/genesis.tgz -o "$ledger_dir"/genesis.tgz; do
        echo "Genesis ledger fetch failed"
        sleep 5
      done
      echo "Fetched genesis ledger in $SECONDS seconds"

      (
        set -x
        cd "$ledger_dir"
        tar -zxf genesis.tgz
        touch .genesis
      )

      (
        if ((boot_from_snapshot)); then
          SECONDS=

          echo "Fetching state snapshot $rpc_url/snapshot.tgz..."
          mkdir -p "$snapshot_dir"
          if ! curl -f "$rpc_url"/snapshot.tgz -o "$snapshot_dir"/snapshot.tgz; then
            echo "State snapshot fetch failed"
            rm -f "$snapshot_dir"/snapshot.tgz
            exit 0  # None fatal
          fi
          echo "Fetched snapshot in $SECONDS seconds"

          SECONDS=
          (
            set -x
            cd "$snapshot_dir"
            tar -zxf snapshot.tgz
            rm snapshot.tgz
          )
          echo "Extracted snapshot in $SECONDS seconds"
        fi
      )
  fi

  [[ -r "$identity_keypair_path" ]] || $solana_keygen new -o "$identity_keypair_path"
  [[ -r "$voting_keypair_path" ]] || $solana_keygen new -o "$voting_keypair_path"
  [[ -r "$storage_keypair_path" ]] || $solana_keygen new -o "$storage_keypair_path"

  vote_pubkey=$($solana_keygen pubkey "$voting_keypair_path")
  storage_pubkey=$($solana_keygen pubkey "$storage_keypair_path")
  identity_pubkey=$($solana_keygen pubkey "$identity_keypair_path")
  export SOLANA_METRICS_HOST_ID="$identity_pubkey"

  setup_validator_accounts "$node_lamports"

  cat <<EOF
======================[ validator configuration ]======================
identity pubkey: $identity_pubkey
vote pubkey: $vote_pubkey
storage pubkey: $storage_pubkey
ledger: $ledger_dir
accounts: $accounts_dir
snapshots: $snapshot_dir
========================================================================
EOF

  echo "$PS4$program ${args[*]}"

  $program "${args[@]}" &
  pid=$!
  echo "pid: $pid"

  if ((no_restart)); then
    wait "$pid"
    exit $?
  fi

  secs_to_next_genesis_poll=5
  while true; do
    if [[ -z $pid ]] || ! kill -0 "$pid"; then
      [[ -z $pid ]] || wait "$pid"
      echo "############## validator exited, restarting ##############"
      break
    fi

    sleep 1

    if ((poll_for_new_genesis_block && --secs_to_next_genesis_poll == 0)); then
      echo "Polling for new genesis block..."
      if new_genesis_block; then
        echo "############## New genesis detected, restarting ##############"
        break
      fi
      secs_to_next_genesis_poll=5
    fi

  done

  kill_node
  # give the cluster time to come back up
  (
    set -x
    sleep 60
  )
done
