#!/bin/bash -e
#
# Perform a quick sanity test on the specific testnet
#

cd "$(dirname "$0")/.."

TESTNET=$1
if [[ -z $TESTNET ]]; then
  TESTNET=testnet.solana.com
fi

echo "--- $TESTNET: wallet sanity"
multinode-demo/test/wallet-sanity.sh $TESTNET

echo --- fin
exit 0
