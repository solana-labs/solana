#!/bin/bash -ex

cd "$(dirname "$0")"/../..

deployMethod="$1"
entrypointIp="$2"
numNodes="$3"
RUST_LOG="$4"

[[ -n $deployMethod ]] || exit
[[ -n $entrypointIp ]] || exit
[[ -n $numNodes ]] || exit

source net/common.sh
loadConfigFile

threadCount=$(nproc)
if [[ $threadCount -gt 4 ]]; then
  threadCount=4
fi

scripts/install-earlyoom.sh

case $deployMethod in
snap)
  rsync -vPrc "$entrypointIp:~/solana/solana.snap" .
  sudo snap install solana.snap --devmode --dangerous
  rm solana.snap

  nodeConfig="\
    leader-ip=$entrypointIp \
    default-metrics-rate=1 \
    metrics-config=$SOLANA_METRICS_CONFIG \
    rust-log=$RUST_LOG \
  "
  # shellcheck disable=SC2086 # Don't want to double quote "$nodeConfig"
  sudo snap set solana $nodeConfig

  solana_bench_tps=/snap/bin/solana.bench-tps
  ;;
local)
  PATH="$HOME"/.cargo/bin:"$PATH"
  export USE_INSTALL=1
  export SOLANA_DEFAULT_METRICS_RATE=1
  export RUST_LOG

  rsync -vPrc "$entrypointIp:~/.cargo/bin/solana*" ~/.cargo/bin/
  solana_bench_tps="multinode-demo/client.sh $entrypointIp:~/solana $entrypointIp:8001"
  ;;
*)
  echo "Unknown deployment method: $deployMethod"
  exit 1
esac

scripts/oom-monitor.sh > oom-monitor.log 2>&1 &

! tmux list-sessions || tmux kill-session

clientCommand="$solana_bench_tps --num-nodes $numNodes --seconds 600 --sustained --threads $threadCount"
tmux new -s solana-bench-tps -d "
  while true; do
    echo === Client start: \$(date) >> client.log
    $metricsWriteDatapoint 'testnet-deploy client-begin=1'
    echo '$ $clientCommand' >> client.log
    $clientCommand >> client.log 2>&1
    $metricsWriteDatapoint 'testnet-deploy client-complete=1'
  done
"
sleep 1
tmux capture-pane -t solana-bench-tps -p -S -100
