#!/bin/bash
set -e

echo "about to show args: "

# Iterate through the command-line arguments
args=()
while [ $# -gt 0 ]; do
  if [[ ${1:0:2} = -- ]]; then
    echo "first arg: $1"
    if [[ $1 == --tpu-enable-udp ]]; then
      args+=($1)
      shift 1
    elif [[ $1 == --tpu-disable-quic ]]; then
      args+=($1)
      shift 1
    else
      echo "Unknown argument: $1"
      exit 1
    fi
  fi
done

for arg in "${args[@]}"; do
  echo "Argument: $arg"
done


/home/solana/k8s-cluster/src/scripts/decode-accounts.sh -t "bootstrap"

# see multinode-demo/boostrap-validator.sh for these default commands
nohup solana-validator \
  --no-os-network-limits-test \
  --no-wait-for-vote-to-start-leader \
  --full-snapshot-interval-slots 200 \
  --identity identity.json \
  --vote-account vote.json \
  --ledger ledger \
  --log logs/solana-validator.log \
  --gossip-host $MY_POD_IP \
  --gossip-port 8001 \
  --rpc-port 8899 \
  --rpc-faucet-address $MY_POD_IP:9900 \
  --no-poh-speed-test \
  --no-incremental-snapshots \
  --full-rpc-api \
  --allow-private-addr \
  "${args[@]}" \
   >logs/init-validator.log 2>&1 &

nohup solana-faucet --keypair faucet.json >logs/faucet.log 2>&1 &

# # Sleep for an hour (3600 seconds)
sleep 3600