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
maybeRequireTower=true

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
    elif [[ $1 = --dev-halt-at-slot ]]; then
      args+=("$1" "$2")
      shift 2
    elif [[ $1 = --dynamic-port-range ]]; then
      args+=("$1" "$2")
      shift 2
    elif [[ $1 = --limit-ledger-size ]]; then
      args+=("$1" "$2")
      shift 2
    elif [[ $1 = --no-rocksdb-compaction ]]; then
      args+=("$1")
      shift
    elif [[ $1 = --enable-rpc-transaction-history ]]; then
      args+=("$1")
      shift
    elif [[ $1 = --rpc-pubsub-enable-block-subscription ]]; then
      args+=("$1")
      shift
    elif [[ $1 = --enable-cpi-and-log-storage ]]; then
      args+=("$1")
      shift
    elif [[ $1 = --enable-extended-tx-metadata-storage ]]; then
      args+=("$1")
      shift
    elif [[ $1 = --enable-rpc-bigtable-ledger-storage ]]; then
      args+=("$1")
      shift
    elif [[ $1 = --tpu-disable-quic ]]; then
      args+=("$1")
      shift
    elif [[ $1 = --tpu-enable-udp ]]; then
      args+=("$1")
      shift
    elif [[ $1 = --rpc-send-batch-ms ]]; then
      args+=("$1" "$2")
      shift 2
    elif [[ $1 = --rpc-send-batch-size ]]; then
      args+=("$1" "$2")
      shift 2
    elif [[ $1 = --skip-poh-verify ]]; then
      args+=("$1")
      shift
    elif [[ $1 = --log ]]; then
      args+=("$1" "$2")
      shift 2
    elif [[ $1 = --no-restart ]]; then
      no_restart=1
      shift
    elif [[ $1 == --wait-for-supermajority ]]; then
      args+=("$1" "$2")
      shift 2
    elif [[ $1 == --expected-bank-hash ]]; then
      args+=("$1" "$2")
      shift 2
    elif [[ $1 == --accounts ]]; then
      args+=("$1" "$2")
      shift 2
    elif [[ $1 == --maximum-snapshots-to-retain ]]; then
      args+=("$1" "$2")
      shift 2
    elif [[ $1 == --no-snapshot-fetch ]]; then
      args+=("$1")
      shift
    elif [[ $1 == --accounts-db-skip-shrink ]]; then
      args+=("$1")
      shift
    elif [[ $1 == --skip-require-tower ]]; then
      maybeRequireTower=false
      shift
    elif [[ $1 = --log-messages-bytes-limit ]]; then
      args+=("$1" "$2")
      shift 2
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
identity=$SOLANA_CONFIG_DIR/bootstrap-validator/identity.json
vote_account="$SOLANA_CONFIG_DIR"/bootstrap-validator/vote-account.json

ledger_dir="$SOLANA_CONFIG_DIR"/bootstrap-validator
[[ -d "$ledger_dir" ]] || {
  echo "$ledger_dir does not exist"
  echo
  echo "Please run: $here/setup.sh"
  exit 1
}

if [[ $maybeRequireTower = true ]]; then
  args+=(--require-tower)
fi

args+=(
  --ledger "$ledger_dir"
  --rpc-port 8899
  --snapshot-interval-slots 200
  --no-incremental-snapshots
  --identity "$identity"
  --vote-account "$vote_account"
  --rpc-faucet-address 127.0.0.1:9900
  --no-poh-speed-test
  --no-os-network-limits-test
  --no-wait-for-vote-to-start-leader
  --full-rpc-api
  --allow-private-addr
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
