#!/bin/bash -e
#
# Perform a quick sanity test on the specific testnet
#

cd "$(dirname "$0")/.."
source multinode-demo/common.sh

NET_URL=$1
if [[ -z $NET_URL ]]; then
  NET_URL=testnet.solana.com
fi

EXPECTED_NODE_COUNT=$2
if [[ -z $EXPECTED_NODE_COUNT ]]; then
  EXPECTED_NODE_COUNT=50
fi


echo "--- $NET_URL: wallet sanity"
multinode-demo/test/wallet-sanity.sh $NET_URL

echo "--- $NET_URL: node count"
if [[ $NET_URL = testnet.solana.com ]]; then
  echo "TODO: Remove this block when a release > 0.7.0 is deployed"
else
  $solana_client_demo $NET_URL $EXPECTED_NODE_COUNT -c
fi

exit 0
