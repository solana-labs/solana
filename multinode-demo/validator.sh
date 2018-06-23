#!/bin/bash
here=$(dirname $0)
. "${here}"/myip.sh

leader=$1

[[ -z ${leader} ]] && {
  echo "usage: $0 [network path to solana repo on leader machine]"
  exit 1
}

myip=$(myip) || exit $?

[[ -f validator-"$myip".json ]] || {
  echo "I can't find a matching validator config file for \"${myip}\"...
Please run ${here}/setup.sh first.
"
  exit 1
}

rsync -vz "${leader}"/{mint-demo.json,leader.json,genesis.log,tx-*.log} . || exit $?

[[ $(uname) = Linux ]] && sudo sysctl -w net.core.rmem_max=26214400 1>/dev/null 2>/dev/null

# if RUST_LOG is unset, default to info
export RUST_LOG=${RUST_LOG:-solana=info}

cargo run --release --bin solana-fullnode -- \
      -l validator-"${myip}".json -v leader.json \
      < genesis.log tx-*.log
