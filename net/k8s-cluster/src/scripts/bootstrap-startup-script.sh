#!/bin/bash
set -e
/home/solana/k8s-cluster/src/scripts/decode-accounts.sh -t "bootstrap"

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
  --full-rpc-api \
  --allow-private-addr \
  >logs/init-validator.log 2>&1 &

nohup solana-faucet --keypair faucet.json >logs/faucet.log 2>&1 &

# # Sleep for an hour (3600 seconds)
sleep 3600