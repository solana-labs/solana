#!/bin/bash
set -e
/home/solana/k8s-cluster/src/scripts/decode-accounts.sh -t "bootstrap"

mkdir -p /home/solana/ledger
tar -xvf /home/solana/genesis/genesis.tar.bz2 -C /home/solana/ledger


# Sleep for an hour (3600 seconds)
sleep 3600