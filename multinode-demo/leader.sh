#!/bin/bash
here=$(dirname $0)
. "${here}"/myip.sh

myip=$(myip) || exit $?

[[ -f leader-"${myip}".json ]] || {
  echo "I can't find a matching leader config file for \"${myip}\"...
Please run ${here}/setup.sh first.
"
  exit 1
}

# if RUST_LOG is unset, default to info
export RUST_LOG=${RUST_LOG:-solana=info}

[[ $(uname) = Linux ]] && sudo sysctl -w net.core.rmem_max=26214400 1>/dev/null 2>/dev/null

# this makes a leader.json file available alongside genesis, etc. for
#  validators and clients
cp leader-"${myip}".json leader.json

cargo run --release --bin solana-fullnode -- \
      -l leader-"${myip}".json \
      < genesis.log tx-*.log \
      > tx-"$(date -u +%Y%m%d%H%M%S%N)".log
