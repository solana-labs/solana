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

echo "--- $NET_URL: verify ledger"
if [[ -z $NO_LEDGER_VERIFY ]]; then
  if [[ -d /var/snap/solana/current/config/ledger ]]; then
    # Note: here we assume this script is actually running on the leader node...
    LEDGER=/tmp/ledger-${BUILDKITE_BUILD_ID:-$$}
    cp -r /var/snap/solana/current/config/ledger "$LEDGER"
    solana.ledger-tool --ledger "$LEDGER" verify
    rm -rf "$LEDGER"
  else
    echo "^^^ +++"
    echo "Ledger verify skipped"
  fi
else
  echo "^^^ +++"
  echo "Ledger verify skipped (NO_LEDGER_VERIFY defined)"
fi

echo "--- $NET_URL: wallet sanity"
(
  set -x
  multinode-demo/test/wallet-sanity.sh $NET_URL
)

echo "--- $NET_URL: node count"
if [[ -n "$USE_SNAP" ]]; then
  # TODO: Merge client.sh functionality into solana-bench-tps proper and
  #       remove this USE_SNAP case
  cmd=$solana_bench_tps
else
  cmd=multinode-demo/client.sh
fi

(
  set -x
  $cmd $NET_URL $EXPECTED_NODE_COUNT -c
)

echo "--- $NET_URL: validator sanity"
if [[ -z $NO_VALIDATOR_SANITY ]]; then
  (
    ./multinode-demo/setup.sh -t validator
    set -e pipefail
    timeout 10s ./multinode-demo/validator.sh "$NET_URL" 2>&1 | tee validator.log
  )
  wc -l validator.log
  if grep -C100 panic validator.log; then
    echo "^^^ +++"
    echo "Panic observed"
    exit 1
  else
    echo "Validator log looks ok"
  fi
else
  echo "^^^ +++"
  echo "Validator sanity disabled (NO_VALIDATOR_SANITY defined)"
fi

exit 0
