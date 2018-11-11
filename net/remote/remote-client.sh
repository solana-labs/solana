#!/usr/bin/env bash -e

cd "$(dirname "$0")"/../..

echo "$(date) | $0 $*" > client.log

deployMethod="$1"
entrypointIp="$2"
RUST_LOG="$3"
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
snap)
  net/scripts/rsync-retry.sh -vPrc "$entrypointIp:~/solana/solana.snap" .
  sudo snap install solana.snap --devmode --dangerous

  solana_bench_tps=/snap/bin/solana.bench-tps
  solana_keygen=/snap/bin/solana.keygen
  ;;
local|tar)
  PATH="$HOME"/.cargo/bin:"$PATH"
  export USE_INSTALL=1
  export SOLANA_DEFAULT_METRICS_RATE=1

  net/scripts/rsync-retry.sh -vPrc "$entrypointIp:~/.cargo/bin/solana*" ~/.cargo/bin/
  solana_bench_tps=solana-bench-tps
  solana_keygen=solana-keygen
  ;;
*)
  echo "Unknown deployment method: $deployMethod"
  exit 1
esac

scripts/oom-monitor.sh > oom-monitor.log 2>&1 &
scripts/net-stats.sh  > net-stats.log 2>&1 &

! tmux list-sessions || tmux kill-session

clientCommand="\
  $solana_bench_tps \
    --network $entrypointIp:8001 \
    --identity client.json \
    --duration 7500 \
    --sustained \
    --threads $threadCount \
"

keygenCommand="$solana_keygen -o client.json"
tmux new -s solana-bench-tps -d "
  [[ -r client.json ]] || {
    echo '$ $keygenCommand'  | tee -a client.log
    $keygenCommand >> client.log 2>&1
  }

  while true; do
    echo === Client start: \$(date) | tee -a client.log
    $metricsWriteDatapoint 'testnet-deploy client-begin=1'
    echo '$ $clientCommand' | tee -a client.log
    $clientCommand >> client.log 2>&1
    $metricsWriteDatapoint 'testnet-deploy client-complete=1'
  done
"
sleep 1
tmux capture-pane -t solana-bench-tps -p -S -100
