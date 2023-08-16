#!/usr/bin/env bash

solana-keygen new --no-bip39-passphrase --silent -o id.json
echo "id: $(solana address -k id.json)"

solana-keygen new --no-bip39-passphrase --silent -o vote.json
echo "vote: $(solana address -k vote.json)"

solana-keygen new --no-bip39-passphrase --silent -o stake.json
echo "stake: $(solana address -k stake.json)"

solana-keygen new --no-bip39-passphrase --silent -o faucet.json
echo "faucet: $(solana address -k faucet.json)"
