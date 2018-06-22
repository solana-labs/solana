#!/bin/bash

if [[ -z $1 ]]; then
  printf 'usage: %s [network path to solana repo on leader machine]\n' "$0"
  exit 1
fi

LEADER=$1

set -x

rsync -v "$LEADER"/{mint-demo.json,leader.json,genesis.log,tx-*.log} . || exit $?

[[ $(uname) = Linux ]] && sudo sysctl -w net.core.rmem_max=26214400

# if RUST_LOG is unset, default to info
export RUST_LOG=${RUST_LOG:-solana=info}

IPADDR="$(ifconfig  | grep 'inet addr:'| grep -v '127.0.0.1' | cut -d: -f2 | awk '{ print $1}')"

if [ -z "$IPADDR" ]; then
    IPADDR="$(ifconfig  | grep 'inet '| grep -v '127.0.0.1' | cut -d: -f2 | awk '{ print $2}')"
fi

cargo run --release --bin solana-fullnode -- \
    -l validator-$IPADDR.json -v leader.json < genesis.log tx-*.log
