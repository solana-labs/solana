#!/bin/bash
set -e
/home/solana/k8s-cluster/src/scripts/decode-accounts.sh -t "validator"

# Sleep for an hour (3600 seconds)
sleep 3600