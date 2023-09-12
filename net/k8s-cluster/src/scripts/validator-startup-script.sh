#!/bin/bash
set -e
/home/solana/k8s-cluster/src/scripts/decode-accounts.sh -t "validator"

solana -u http://$BOOTSTRAP_RPC_PORT airdrop 500 identity.json
solana -u http://$BOOTSTRAP_RPC_PORT create-vote-account --allow-unsafe-authorized-withdrawer vote.json identity.json identity.json -k identity.json
solana -u http://$BOOTSTRAP_RPC_PORT create-stake-account stake.json 1.00228288 -k identity.json
solana -u http://$BOOTSTRAP_RPC_PORT delegate-stake stake.json vote.json --force -k identity.json

nohup solana-validator \
  --no-os-network-limits-test \
  --identity identity.json \
  --vote-account vote.json \
  --entrypoint $BOOTSTRAP_GOSSIP_PORT \
  --rpc-faucet-address $BOOTSTRAP_FAUCET_PORT \
  --gossip-port 8001 \
  --rpc-port 8899 \
  --ledger ledger \
  --log logs/solana-validator.log \
  --full-rpc-api \
  --allow-private-addr \
  >logs/init-validator.log 2>&1 &


# # Sleep for an hour (3600 seconds)
sleep 3600