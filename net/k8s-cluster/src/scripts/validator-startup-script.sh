#!/bin/bash
set -e
/home/solana/k8s-cluster/src/scripts/decode-accounts.sh -t "validator"

# nohup solana-validator \
#   --no-os-network-limits-test \
#   --identity identity.json \
#   --vote-account vote.json \
#   --entrypoint $BOOTSTRAP_GOSSIP_PORT \
#   --rpc-faucet-address $BOOTSTRAP_FAUCET_PORT \
#   --gossip-port 8001 \
#   --rpc-port 8899 \
#   --ledger ledger \
#   --log logs/solana-validator.log \
#   --full-rpc-api \
#   --allow-private-addr \
#   >logs/init-validator.log 2>&1 &


# # Sleep for an hour (3600 seconds)
sleep 3600