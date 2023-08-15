#!/usr/bin/env bash

nohup solana-faucet --keypair faucet.json >logs/faucet.log 2>&1 &
