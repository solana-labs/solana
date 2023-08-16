#!/usr/bin/env bash

solana-keygen new --no-bip39-passphrase --silent -o id.json
echo "id: $(solana address -k id.json)"

solana-keygen new --no-bip39-passphrase --silent -o vote.json
echo "vote: $(solana address -k vote.json)"

solana-keygen new --no-bip39-passphrase --silent -o stake.json
echo "stake: $(solana address -k stake.json)"

mkdir -p /workspace/logs-info
echo "info: $BOOTSTRAP_GOSSIP_PORT, $BOOTSTRAP_RPC_PORT, $BOOTSTRAP_FAUCET_PORT" > /workspace/logs-info/test.log

solana -u http://$BOOTSTRAP_RPC_PORT airdrop 500 id.json
solana -u http://$BOOTSTRAP_RPC_PORT create-vote-account --allow-unsafe-authorized-withdrawer vote.json id.json id.json -k id.json
solana -u http://$BOOTSTRAP_RPC_PORT create-stake-account stake.json 1.00228288 -k id.json
solana -u http://$BOOTSTRAP_RPC_PORT delegate-stake stake.json vote.json --force -k id.json