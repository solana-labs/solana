#!/bin/bash -e
#
# Perform a quick sanity test on the specific testnet
#

cd "$(dirname "$0")/.."
source multinode-demo/common.sh

TESTNET=$1
if [[ -z $TESTNET ]]; then
  TESTNET=testnet.solana.com
fi

echo "--- $TESTNET: wallet sanity"
multinode-demo/test/wallet-sanity.sh $TESTNET

echo "--- $TESTNET: node count"
if [[ $TESTNET = testnet.solana.com ]]; then
  echo "TODO: Remove this block when a release > 0.7.0 is deployed"
else
  $solana_client_demo $TESTNET 50 -c  # <-- Expect to see at least 50 nodes
fi

exit 0
