#!/bin/bash -e

deployMethod="$1"
netEntrypoint="$2"
numNodes="$3"
RUST_LOG="$4"
[[ -n $deployMethod ]] || exit
[[ -n $netEntrypoint ]] || exit
[[ -n $numNodes ]] || exit

cd "$(dirname "$0")"/../..
source net/common.sh
loadConfigFile

threadCount=$(nproc)
if [[ $threadCount -gt 4 ]]; then
  threadCount=4
fi

./script/install-earlyoom.sh

case $deployMethod in
snap)
  sudo snap install solana.snap --devmode --dangerous
  rm solana.snap

  sudo snap set solana metrics-config="$SOLANA_METRICS_CONFIG" rust-log="$RUST_LOG"
  solana_bench_tps=/snap/bin/solana.bench-tps
  ;;
local)
  PATH="$HOME"/.cargo/bin:"$PATH"
  export USE_INSTALL=1
  export RUST_LOG

  rsync -vPrz "$netEntrypoint:~/.cargo/bin/solana*" ~/.cargo/bin/
  solana_bench_tps=multinode-demo/client.sh
  netEntrypoint="$:~/solana"
  ;;
*)
  echo "Unknown deployment method: $deployMethod"
  exit 1
esac

./scripts/oom-monitor.sh  > oom-monitor.log 2>&1 &

while true; do
  echo "=== Client start: $(date)" >> client.log
  clientCommand="$solana_bench_tps $netEntrypoint $numNodes --loop -s 600 --sustained -t threadCount"
  echo "$ $clientCommand" >> client.log

  $clientCommand >> client.log 2>&1

  $metricsWriteDatapoint "testnet-deploy,name=$netBasename clientexit=1"
  echo Error: bench-tps should never exit | tee -a client.log
done

