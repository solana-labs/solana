#!/usr/bin/env bash
#
# Starts an instance of solana-drone
#
here=$(dirname "$0")

# shellcheck source=multinode-demo/common.sh
source "$here"/common.sh

usage () {
  exitcode=0
  if [[ -n "$1" ]]; then
    exitcode=1
    echo "Error: $*"
  fi
  cat <<EOF
usage: $0 [-c lamport_cap]

Run an airdrop drone

 -n lamport_cap    - Airdrop cap in lamports

EOF
  exit $exitcode
}

lamport_cap=1000000000

while getopts "h?c:" opt; do
  case $opt in
  h|\?)
    usage
    exit 0
    ;;
  c)
    lamport_cap="$OPTARG"
    ;;
  *)
    usage "Error: unhandled option: $opt"
    ;;
  esac
done

[[ -f "$SOLANA_CONFIG_DIR"/mint-id.json ]] || {
  echo "$SOLANA_CONFIG_DIR/mint-id.json not found, create it by running:"
  echo
  echo "  ${here}/setup.sh"
  exit 1
}

set -ex

trap 'kill "$pid" && wait "$pid"' INT TERM ERR
$solana_drone \
  --cap "$lamport_cap" \
  --keypair "$SOLANA_CONFIG_DIR"/mint-id.json \
  > >($drone_logger) 2>&1 &
pid=$!
wait "$pid"
