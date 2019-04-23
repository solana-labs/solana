#!/usr/bin/env bash
set -e

cd "$(dirname "$0")"/../..

echo "$(date) | $0 $*" > client.log

deployMethod="$1"
entrypointIp="$2"
clientToRun="$3"
RUST_LOG="$4"
benchTpsExtraArgs="$5"
benchExchangeExtraArgs="$6"
export RUST_LOG=${RUST_LOG:-solana=info} # if RUST_LOG is unset, default to info

missing() {
  echo "Error: $1 not specified"
  exit 1
}

[[ -n $deployMethod ]] || missing deployMethod
[[ -n $entrypointIp ]] || missing entrypointIp

source net/common.sh
loadConfigFile

threadCount=$(nproc)
if [[ $threadCount -gt 4 ]]; then
  threadCount=4
fi

case $deployMethod in
local|tar)
  PATH="$HOME"/.cargo/bin:"$PATH"
  export USE_INSTALL=1

  ./fetch-perf-libs.sh
  # shellcheck source=/dev/null
  source ./target/perf-libs/env.sh

  net/scripts/rsync-retry.sh -vPrc "$entrypointIp:~/.cargo/bin/solana*" ~/.cargo/bin/
  ;;
*)
  echo "Unknown deployment method: $deployMethod"
  exit 1
esac

(
  sudo scripts/oom-monitor.sh
) > oom-monitor.log 2>&1 &
scripts/net-stats.sh  > net-stats.log 2>&1 &

! tmux list-sessions || tmux kill-session

case $clientToRun in
solana-bench-tps)
  clientCommand="\
    solana-bench-tps \
      --network $entrypointIp:8001 \
      --drone $entrypointIp:9900 \
      --duration 7500 \
      --sustained \
      --threads $threadCount \
      $benchTpsExtraArgs \
  "
  ;;
solana-bench-exchange)
  solana-keygen -o bench.keypair
  clientCommand="\
    solana-bench-exchange \
      --network $entrypointIp:8001 \
      --drone $entrypointIp:9900 \
      --threads $threadCount \
      --batch-size 1000 \
      --fund-amount 20000 \
      --duration 7500 \
      --identity bench.keypair \
      $benchExchangeExtraArgs \
  "
  ;;
*)
  echo "Unknown client name: $clientToRun"
  exit 1
esac

tmux new -s "$clientToRun" -d "
  while true; do
    echo === Client start: \$(date) | tee -a client.log
    $metricsWriteDatapoint 'testnet-deploy client-begin=1'
    echo '$ $clientCommand' | tee -a client.log
    $clientCommand >> client.log 2>&1
    $metricsWriteDatapoint 'testnet-deploy client-complete=1'
  done
"
sleep 1
tmux capture-pane -t "$clientToRun" -p -S -100
