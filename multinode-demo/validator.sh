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
  --node-lamports LAMPORTS  - Number of lamports this node has been funded from the genesis config
  --no-voting               - start node without vote signer
  --rpc-port port           - custom RPC port for this node
  --no-restart              - do not restart the node if it exits
  --no-airdrop              - The genesis config has an account for the node. Airdrops are not required.

EOF
  exit 1
}

args=()
airdrops_enabled=1
node_lamports=500000000000 # 500 SOL: number of lamports to airdrop the node for transaction fees (ignored if airdrops_enabled=0)
label=
identity_keypair_path=
voting_keypair_path=
no_restart=0
gossip_entrypoint=
ledger_dir=

positional_args=()
while [[ -n $1 ]]; do
  if [[ ${1:0:1} = - ]]; then
    # validator.sh-only options
    if [[ $1 = --label ]]; then
      label="-$2"
      shift 2
    elif [[ $1 = --no-restart ]]; then
      no_restart=1
      shift
    elif [[ $1 = --node-lamports ]]; then
      node_lamports="$2"
      shift 2
    elif [[ $1 = --no-airdrop ]]; then
      airdrops_enabled=0
      shift
    # solana-validator options
    elif [[ $1 = --blockstream ]]; then
      args+=("$1" "$2")
      shift 2
    elif [[ $1 = --expected-genesis-hash ]]; then
      args+=("$1" "$2")
      shift 2
    elif [[ $1 = --identity-keypair ]]; then
      identity_keypair_path=$2
      args+=("$1" "$2")
      shift 2
    elif [[ $1 = --voting-keypair ]]; then
      voting_keypair_path=$2
      args+=("$1" "$2")
      shift 2
    elif [[ $1 = --vote-account ]]; then
      args+=("$1" "$2")
      shift 2
    elif [[ $1 = --storage-keypair ]]; then
      storage_keypair_path=$2
      args+=("$1" "$2")
      shift 2
    elif [[ $1 = --init-complete-file ]]; then
      args+=("$1" "$2")
      shift 2
    elif [[ $1 = --ledger ]]; then
      ledger_dir=$2
      shift 2
    elif [[ $1 = --entrypoint ]]; then
      gossip_entrypoint=$2
      args+=("$1" "$2")
      shift 2
    elif [[ $1 = --no-snapshot-fetch ]]; then
      args+=("$1")
      shift
    elif [[ $1 = --no-voting ]]; then
      args+=("$1")
      shift
    elif [[ $1 = --dev-no-sigverify ]]; then
      args+=("$1")
      shift
    elif [[ $1 = --dev-halt-at-slot ]]; then
      args+=("$1" "$2")
      shift 2
    elif [[ $1 = --rpc-port ]]; then
      args+=("$1" "$2")
      shift 2
    elif [[ $1 = --enable-rpc-exit ]]; then
      args+=("$1")
      shift
    elif [[ $1 = --rpc-drone-address ]]; then
      args+=("$1" "$2")
      shift 2
    elif [[ $1 = --vote-signer-address ]]; then
      args+=("$1" "$2")
      shift 2
    elif [[ $1 = --accounts ]]; then
      args+=("$1" "$2")
      shift 2
    elif [[ $1 = --gossip-port ]]; then
      args+=("$1" "$2")
      shift 2
    elif [[ $1 = --dynamic-port-range ]]; then
      args+=("$1" "$2")
      shift 2
    elif [[ $1 = --snapshot-interval-slots ]]; then
      args+=("$1" "$2")
      shift 2
    elif [[ $1 = --limit-ledger-size ]]; then
      args+=("$1")
      shift
    elif [[ $1 = --skip-poh-verify ]]; then
      args+=("$1")
      shift
    elif [[ $1 = --log ]]; then
      args+=("$1" "$2")
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

if [[ "$SOLANA_GPU_MISSING" -eq 1 ]]; then
  echo "Testnet requires GPUs, but none were found!  Aborting..."
  exit 1
fi

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
    usage "Error: --identity-keypair not specified"
  fi
  if [[ -z $voting_keypair_path ]]; then
    usage "Error: --voting-keypair not specified"
  fi
fi

if [[ -z "$ledger_dir" ]]; then
  ledger_dir="$SOLANA_CONFIG_DIR/validator$label"
fi
mkdir -p "$ledger_dir"

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

drone_address="${gossip_entrypoint%:*}":9900

: "${identity_keypair_path:=$ledger_dir/identity-keypair.json}"
: "${voting_keypair_path:=$ledger_dir/vote-keypair.json}"
: "${storage_keypair_path:=$ledger_dir/storage-keypair.json}"

default_arg --entrypoint "$gossip_entrypoint"
if ((airdrops_enabled)); then
  default_arg --rpc-drone-address "$drone_address"
fi

default_arg --identity-keypair "$identity_keypair_path"
default_arg --voting-keypair "$voting_keypair_path"
default_arg --storage-keypair "$storage_keypair_path"
default_arg --ledger "$ledger_dir"
default_arg --log -

if [[ -n $SOLANA_CUDA ]]; then
  program=$solana_validator_cuda
else
  program=$solana_validator
fi

if [[ -z $CI ]]; then # Skip in CI
  # shellcheck source=scripts/tune-system.sh
  source "$here"/../scripts/tune-system.sh
fi

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
}

kill_node_and_exit() {
  kill_node
  exit
}

trap 'kill_node_and_exit' INT TERM ERR

wallet() {
  (
    set -x
    $solana_cli --keypair "$identity_keypair_path" --url "$rpc_url" "$@"
  )
}

setup_validator_accounts() {
  declare node_lamports=$1

  if ! wallet show-vote-account "$voting_keypair_path"; then
    if ((airdrops_enabled)); then
      echo "Adding $node_lamports to validator identity account:"
      wallet airdrop "$node_lamports" lamports || return $?
    fi

    echo "Creating validator vote account"
    wallet create-vote-account "$voting_keypair_path" "$identity_keypair_path" --commission 50 || return $?
  fi
  echo "Validator vote account configured"

  if ! wallet show-storage-account "$storage_keypair_path"; then
    echo "Creating validator storage account"
    wallet create-validator-storage-account "$identity_keypair_path" "$storage_keypair_path" || return $?
  fi
  echo "Validator storage account configured"

  echo "Validator identity account balance:"
  wallet balance --lamports || return $?

  return 0
}

rpc_url=$($solana_gossip get-rpc-url --entrypoint "$gossip_entrypoint")

[[ -r "$identity_keypair_path" ]] || $solana_keygen new --no-passphrase -so "$identity_keypair_path"
[[ -r "$voting_keypair_path" ]] || $solana_keygen new --no-passphrase -so "$voting_keypair_path"
[[ -r "$storage_keypair_path" ]] || $solana_keygen new --no-passphrase -so "$storage_keypair_path"

setup_validator_accounts "$node_lamports"

while true; do
  echo "$PS4$program ${args[*]}"

  $program "${args[@]}" &
  pid=$!
  echo "pid: $pid"

  if ((no_restart)); then
    wait "$pid"
    exit $?
  fi

  while true; do
    if [[ -z $pid ]] || ! kill -0 "$pid"; then
      echo "############## validator exited, restarting ##############"
      break
    fi
    sleep 1
  done

  kill_node
done
