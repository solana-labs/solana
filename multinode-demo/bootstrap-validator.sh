#!/usr/bin/env bash
#
# Start the bootstrap validator node
#
set -e

here=$(dirname "$0")
# shellcheck source=multinode-demo/common.sh
source "$here"/common.sh

if [[ "$SOLANA_GPU_MISSING" -eq 1 ]]; then
  echo "Testnet requires GPUs, but none were found!  Aborting..."
  exit 1
fi

if [[ -n $SOLANA_CUDA ]]; then
  program=$solana_validator_cuda
else
  program=$solana_validator
fi

no_restart=0

args=()
while [[ -n $1 ]]; do
  if [[ ${1:0:1} = - ]]; then
    if [[ $1 = --init-complete-file ]]; then
      args+=("$1" "$2")
      shift 2
    elif [[ $1 = --gossip-host ]]; then
      args+=("$1" "$2")
      shift 2
    elif [[ $1 = --gossip-port ]]; then
      args+=("$1" "$2")
      shift 2
    elif [[ $1 = --dynamic-port-range ]]; then
      args+=("$1" "$2")
      shift 2
    elif [[ $1 = --limit-ledger-size ]]; then
      args+=("$1")
      shift
    elif [[ $1 = --enable-rpc-get-confirmed-block ]]; then
      args+=("$1")
      shift
    elif [[ $1 = --skip-poh-verify ]]; then
      args+=("$1")
      shift
    elif [[ $1 = --log ]]; then
      args+=("$1" "$2")
      shift 2
    elif [[ $1 = --no-restart ]]; then
      no_restart=1
      shift
    else
      echo "Unknown argument: $1"
      $program --help
      exit 1
    fi
  else
    echo "Unknown argument: $1"
    $program --help
    exit 1
  fi
done

# These keypairs are created by ./setup.sh and included in the genesis config
identity_keypair=$SOLANA_CONFIG_DIR/bootstrap-validator/identity-keypair.json
vote_keypair="$SOLANA_CONFIG_DIR"/bootstrap-validator/vote-keypair.json

ledger_dir="$SOLANA_CONFIG_DIR"/bootstrap-validator
[[ -d "$ledger_dir" ]] || {
  echo "$ledger_dir does not exist"
  echo
  echo "Please run: $here/setup.sh"
  exit 1
}

args+=(
  --enable-rpc-exit
  --enable-rpc-set-log-filter
  --ledger "$ledger_dir"
  --rpc-port 8899
  --snapshot-interval-slots 100
  --identity-keypair "$identity_keypair"
  --voting-keypair "$vote_keypair"
  --rpc-faucet-address 127.0.0.1:9900
)
default_arg --gossip-port 8001
default_arg --log -


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

while true; do
  echo "$program ${args[*]}"
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
