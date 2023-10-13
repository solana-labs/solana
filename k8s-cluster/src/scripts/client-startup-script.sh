#!/bin/bash
set -e

/home/solana/k8s-cluster-scripts/decode-accounts.sh -t "client"

clientToRun="$1"
benchTpsExtraArgs="$2"
clientIndex="$3"
clientType="${4:-thin-client}"

missing() {
  echo "Error: $1 not specified"
  exit 1
}

[[ -n $deployMethod ]] || missing deployMethod
[[ -n $entrypointIp ]] || missing entrypointIp

threadCount=$(nproc)
if [[ $threadCount -gt 4 ]]; then
  threadCount=4
fi

TPU_CLIENT=false
RPC_CLIENT=false
case "$clientType" in
  thin-client)
    TPU_CLIENT=false
    RPC_CLIENT=false
    ;;
  tpu-client)
    TPU_CLIENT=true
    RPC_CLIENT=false
    ;;
  rpc-client)
    TPU_CLIENT=false
    RPC_CLIENT=true
    ;;
  *)
    echo "Unexpected clientType: \"$clientType\""
    exit 1
    ;;
esac

case $clientToRun in
solana-bench-tps)
  net/scripts/rsync-retry.sh -vPrc \
    "$entrypointIp":~/solana/config/bench-tps"$clientIndex".yml ./client-accounts.yml

  args=()

  if ${TPU_CLIENT}; then
    args+=(--use-tpu-client)
    args+=(--url "$BOOTSTRAP_RPC_ADDRESS")
  elif ${RPC_CLIENT}; then
    args+=(--use-rpc-client)
    args+=(--url "$BOOTSTRAP_RPC_ADDRESS")
  else
    args+=(--entrypoint "$BOOTSTRAP_GOSSIP_ADDRESS")
  fi

  clientCommand="\
    solana-bench-tps \
      --duration 7500 \
      --sustained \
      --threads $threadCount \
      $benchTpsExtraArgs \
      --read-client-keys ./client-accounts.yml \
      ${args[*]} \
  "
  ;;
idle)
  # In net/remote/remote-client.sh, we add faucet keypair here
  # but in this case we already do that in the docker container
  # by default
  exit 0
  ;;
*)
  echo "Unknown client name: $clientToRun"
  exit 1
esac


echo "$(date) | $0 $*" >> /home/solana/logs/client.log

! tmux list-sessions || tmux kill-session

tmux new -s "$clientToRun" -d "
  while true; do
    echo === Client start: \$(date) | tee -a /home/solana/logs/client.log
    echo '$ $clientCommand' | tee -a /home/solana/logs/client.log
    $clientCommand >> /home/solana/logs/client.log 2>&1
  done
"

sleep 1

tmux capture-pane -t "$clientToRun" -p -S -100