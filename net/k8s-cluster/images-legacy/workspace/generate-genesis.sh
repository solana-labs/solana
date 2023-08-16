#!/usr/bin/env bash

solana-genesis \
  --enable-warmup-epochs \
  --bootstrap-validator id.json vote.json stake.json \
  --bootstrap-validator-stake-lamports 10002282880 \
  --max-genesis-archive-unpacked-size 1073741824 \
  --faucet-pubkey faucet.json \
  --faucet-lamports 500000000000000000 \
  --hashes-per-tick auto \
  --cluster-type development \
  --ledger ledger
