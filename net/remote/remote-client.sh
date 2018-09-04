#!/bin/bash -ex

cd "$(dirname "$0")"/../..

deployMethod="$1"
leaderIp="$2"
numNodes="$3"
RUST_LOG="$4"
[[ -n $deployMethod ]] || exit
[[ -n $leaderIp ]] || exit
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
  rsync -vPrc "$leaderIp:~/solana/solana.snap" .
  sudo snap install solana.snap --devmode --dangerous
  rm solana.snap

  nodeConfig="\
    leader-ip=$leaderIp \
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

  rsync -vPrc "$leaderIp:~/.cargo/bin/solana*" ~/.cargo/bin/
  solana_bench_tps="multinode-demo/client.sh $leaderIp:~/solana"
  ;;
*)
  echo "Unknown deployment method: $deployMethod"
  exit 1
esac

scripts/oom-monitor.sh > oom-monitor.log 2>&1 &

while true; do
  echo "=== Client start: $(date)" >> client.log
  clientCommand="$solana_bench_tps --num-nodes $numNodes --seconds 600 --sustained --threads $threadCount"
  echo "$ $clientCommand" >> client.log

  set +e
  $clientCommand >> client.log 2>&1
  set -e

  $metricsWriteDatapoint "testnet-deploy,name=$netBasename clientexit=1"
  echo Error: bench-tps should never exit | tee -a client.log
done

