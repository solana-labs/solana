#!/usr/bin/env bash

echo "info in validator.sh: $BOOTSTRAP_GOSSIP_PORT, $BOOTSTRAP_FAUCET_PORT" > /workspace/logs-info/bruh.log

nohup solana-validator \
  --no-os-network-limits-test \
  --identity id.json \
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
