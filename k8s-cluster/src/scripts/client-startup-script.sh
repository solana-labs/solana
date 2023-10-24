#!/bin/bash
set -e

SECRET_FILE=(
  "/home/solana/${validator_type}-accounts/faucet.base64"
)
DECODED_FILE=(
  "/home/solana/faucet.json"
)

# Check if the secret file exists
if [ -f "$SECRET_FILE" ]; then
    echo "Secret file found at $SECRET_FILE"

    # Read and decode the base64-encoded secret
    SECRET_CONTENT=$(base64 -d < "$SECRET_FILE")

    # Save the decoded secret content to a file
    echo "$SECRET_CONTENT" > "$DECODED_FILE"
    echo "Decoded secret content saved to $DECODED_FILE"
else
    echo "Secret file not found at $SECRET_FILE"
fi

mkdir -p /home/solana/logs


clientToRun="$1"
benchTpsExtraArgs="$2"
clientType="${3:-thin-client}"

echo "client to run: $1"
echo "benchtpsextraargs: $2"
echo "client type: $3"


missing() {
  echo "Error: $1 not specified"
  exit 1
}

# [[ -n $deployMethod ]] || missing deployMethod
# [[ -n $entrypointIp ]] || missing entrypointIp

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
bench-tps)
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

sleep 3600

# cat > ~/solana/on-reboot <<EOF
# #!/usr/bin/env bash
# cd ~/solana

# PATH="$HOME"/.cargo/bin:"$PATH"
# export USE_INSTALL=1

# echo "$(date) | $0 $*" >> client.log

# ! tmux list-sessions || tmux kill-session

# tmux new -s "$clientToRun" -d "
#   while true; do
#     echo === Client start: \$(date) | tee -a client.log
#     $metricsWriteDatapoint 'testnet-deploy client-begin=1'
#     echo '$ $clientCommand' | tee -a client.log
#     $clientCommand >> client.log 2>&1
#     $metricsWriteDatapoint 'testnet-deploy client-complete=1'
#   done
# "
# EOF
# chmod +x ~/solana/on-reboot
# echo "@reboot ~/solana/on-reboot" | crontab -

# ~/solana/on-reboot

# sleep 1
# tmux capture-pane -t "$clientToRun" -p -S -100


echo "$(date) | $0 $*" >> /home/solana/logs/client.log

tmux &

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