#!/usr/bin/env bash
set -e

cd "$(dirname "$0")"/../..

deployMethod="$1"
entrypointIp="$2"
clientToRun="$3"
if [[ -n $4 ]]; then
  export RUST_LOG="$4"
fi
benchTpsExtraArgs="$5"
benchExchangeExtraArgs="$6"
clientIndex="$7"

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
skip)
  ;;
*)
  echo "Unknown deployment method: $deployMethod"
  exit 1
esac

case $clientToRun in
solana-bench-tps)
  net/scripts/rsync-retry.sh -vPrc \
    "$entrypointIp":~/solana/solana-client-accounts/bench-tps"$clientIndex".yml ./client-accounts.yml
  clientCommand="\
    solana-bench-tps \
      --entrypoint $entrypointIp:8001 \
      --drone $entrypointIp:9900 \
      --duration 7500 \
      --sustained \
      --threads $threadCount \
      $benchTpsExtraArgs \
      --read-client-keys ./client-accounts.yml \
  "
  ;;
solana-bench-exchange)
  solana-keygen new -f -o bench.keypair
  net/scripts/rsync-retry.sh -vPrc \
    "$entrypointIp":~/solana/solana-client-accounts/bench-exchange"$clientIndex".yml ./client-accounts.yml
  clientCommand="\
    solana-bench-exchange \
      --entrypoint $entrypointIp:8001 \
      --drone $entrypointIp:9900 \
      --threads $threadCount \
      --batch-size 1000 \
      --fund-amount 20000 \
      --duration 7500 \
      --identity bench.keypair \
      $benchExchangeExtraArgs \
      --read-client-keys ./client-accounts.yml \
  "
  ;;
*)
  echo "Unknown client name: $clientToRun"
  exit 1
esac


cat > ~/solana/on-reboot <<EOF
#!/usr/bin/env bash
cd ~/solana

PATH="$HOME"/.cargo/bin:"$PATH"
export USE_INSTALL=1

echo "$(date) | $0 $*" >> client.log

(
  sudo SOLANA_METRICS_CONFIG="$SOLANA_METRICS_CONFIG" scripts/oom-monitor.sh
) > oom-monitor.log 2>&1 &
echo $! > oom-monitor.pid
scripts/fd-monitor.sh > fd-monitor.log 2>&1 &
echo $! > fd-monitor.pid
scripts/net-stats.sh  > net-stats.log 2>&1 &
echo $! > net-stats.pid
! tmux list-sessions || tmux kill-session

tmux new -s "$clientToRun" -d "
  while true; do
    echo === Client start: \$(date) | tee -a client.log
    $metricsWriteDatapoint 'testnet-deploy client-begin=1'
    echo '$ $clientCommand' | tee -a client.log
    $clientCommand >> client.log 2>&1
    $metricsWriteDatapoint 'testnet-deploy client-complete=1'
  done
"
EOF
chmod +x ~/solana/on-reboot
echo "@reboot ~/solana/on-reboot" | crontab -

~/solana/on-reboot

sleep 1
tmux capture-pane -t "$clientToRun" -p -S -100
