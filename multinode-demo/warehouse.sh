#!/usr/bin/env bash

usage() {
  declare exitcode=0
  if [[ -n $1 ]]; then
    echo "$@"
    exitcode=1
  fi

  cat <<EOF
Usage:
  $0 [--entrypoint <gossip entry point>] [--expected-genesis-hash <genesis hash>]

Demonstration of how to periodically warehouse the entire ledger into a series
of snapshots for longer-term storage

EOF
  exit $exitcode
}

set -e

here=$(dirname "$0")
# shellcheck source=multinode-demo/common.sh
source "$here"/common.sh

args=()
archive_interval_seconds=60
ledger_warehouse_dir="$here"/warehouse
ledger_dir="$SOLANA_CONFIG_DIR"/warehouse-ledger

while [[ -n $1 ]]; do
  if [[ $1 = --entrypoint ]]; then
    args+=("$1" "$2")
    shift 2
  elif [[ $1 = --expected-genesis-hash ]]; then
    args+=("$1" "$2")
    shift 2
  elif [[ $1 = --skip-poh-verify ]]; then
    # It can be convenient during testing to supply the --skip-poh-verify
    # argument to speed up validator boot times.  Not recommended for production
    args+=("$1")
    shift 1
  elif [[ $1 = -h ]] || [[ $1 = --help ]]; then
    usage
  else
    usage "Unknown argument: $1"
  fi
done

args+=(
  --ledger "$ledger_dir" \
  --no-snapshot-fetch \
)
default_arg --entrypoint testnet.solana.com:8001

echo "Warehouse directory: $ledger_warehouse_dir"
echo "Archive interval: $archive_interval_seconds seconds"
mkdir -p "$ledger_warehouse_dir"

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

last_snapshot=
while true; do
  # shellcheck disable=SC2086 # Don't want to double quote $solana_validator
  $solana_validator "${args[@]}" &
  pid=$!
  echo "pid: $pid"

  secs_to_next_ledger_archive=$archive_interval_seconds
  while true; do
    if [[ -z $pid ]] || ! kill -0 "$pid"; then
      # validator exited unexpectedly, restart it
      break;
    else
      sleep 1

      if ((--secs_to_next_ledger_archive == 0)); then
        if [[ ! -f "$ledger_dir"/snapshot.tar.bz2 ]]; then
          echo "Validator has not produced a snapshot yet"
          secs_to_next_ledger_archive=60 # try again later
          continue
        fi

        if [[ -f "$last_snapshot" ]] && diff "$ledger_dir"/snapshot.tar.bz2 "$last_snapshot"; then
          echo "Validator has not produced a new snapshot yet"
          secs_to_next_ledger_archive=60 # try again later
          continue
        fi

        kill_node

        # Move the current ledger into the warehouse
        archive_dir="$ledger_warehouse_dir/$(date -u +'%Y-%m-%dT%H:%M:%SZ')"
        echo "Archiving current ledger to $archive_dir"
        mv "$ledger_dir" "$archive_dir"
        last_snapshot="$archive_dir/snapshot.tar.bz2"
        test -f "$last_snapshot"

        # Construct a new ledger consisting of genesis and the latest snapshot
        mkdir -p "$ledger_dir"
        cp "$archive_dir"/genesis.tar.bz2 "$ledger_dir"
        cp "$archive_dir"/snapshot.tar.bz2 "$ledger_dir"
        tar -C "$ledger_dir" -jxf "$ledger_dir"/genesis.tar.bz2
        break
      fi
    fi
  done
done
