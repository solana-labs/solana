#!/usr/bin/env bash
set -e
#
# This script is to be run on the bootstrap full node
#

cd "$(dirname "$0")"/../..

deployMethod=
entrypointIp=
numNodes=

[[ -r deployConfig ]] || {
  echo deployConfig missing
  exit 1
}
# shellcheck source=/dev/null # deployConfig is written by remote-node.sh
source deployConfig

missing() {
  echo "Error: $1 not specified"
  exit 1
}

[[ -n $deployMethod ]]   || missing deployMethod
[[ -n $entrypointIp ]]   || missing entrypointIp
[[ -n $numNodes ]]       || missing numNodes
[[ -n $leaderRotation ]] || missing leaderRotation

ledgerVerify=true
validatorSanity=true
rejectExtraNodes=false
while [[ $1 = -o ]]; do
  opt="$2"
  shift 2
  case $opt in
  noLedgerVerify)
    ledgerVerify=false
    ;;
  noValidatorSanity)
    validatorSanity=false
    ;;
  rejectExtraNodes)
    rejectExtraNodes=true
    ;;
  *)
    echo "Error: unknown option: $opt"
    exit 1
    ;;
  esac
done

RUST_LOG="$1"
export RUST_LOG=${RUST_LOG:-solana=info} # if RUST_LOG is unset, default to info

source net/common.sh
loadConfigFile

case $deployMethod in
local|tar)
  PATH="$HOME"/.cargo/bin:"$PATH"
  export USE_INSTALL=1
  if [[ -r target/perf-libs/env.sh ]]; then
    # shellcheck source=/dev/null
    source target/perf-libs/env.sh
  fi

  entrypointRsyncUrl="$entrypointIp:~/solana"

  solana_gossip=solana-gossip
  solana_ledger_tool=solana-ledger-tool
  solana_keygen=solana-keygen

  ledger=config-local/bootstrap-leader-ledger
  client_id=config-local/client-id.json
  ;;
*)
  echo "Unknown deployment method: $deployMethod"
  exit 1
esac

echo "+++ $entrypointIp: node count ($numNodes expected)"
(
  set -x
  $solana_keygen -o "$client_id"

  nodeArg="num-nodes"
  if $rejectExtraNodes; then
    nodeArg="num-nodes-exactly"
  fi

  timeout 2m $solana_gossip \
    --network "$entrypointIp:8001" \
    --$nodeArg "$numNodes" \
)

echo "--- RPC API: getTransactionCount"
(
  set -x
  curl --retry 5 --retry-delay 2 --retry-connrefused \
    -X POST -H 'Content-Type: application/json' \
    -d '{"jsonrpc":"2.0","id":1, "method":"getTransactionCount"}' \
    http://"$entrypointIp":8899
)

echo "--- $entrypointIp: wallet sanity"
(
  set -x
  scripts/wallet-sanity.sh --host "$entrypointIp"
)

echo "--- $entrypointIp: verify ledger"
if $ledgerVerify; then
  if [[ -d $ledger ]]; then
    (
      set -x
      rm -rf /var/tmp/ledger-verify
      du -hs "$ledger"
      time cp -r "$ledger" /var/tmp/ledger-verify
      time $solana_ledger_tool --ledger /var/tmp/ledger-verify verify
    )
  else
    echo "^^^ +++"
    echo "Ledger verify skipped: directory does not exist: $ledger"
  fi
else
  echo "^^^ +++"
  echo "Note: ledger verify disabled"
fi


echo "--- $entrypointIp: validator sanity"
if $validatorSanity; then
  (
    set -x -o pipefail
    timeout 10s ./multinode-demo/fullnode-x.sh --stake 0 \
      "$entrypointRsyncUrl" \
      "$entrypointIp:8001" 2>&1 | tee validator-sanity.log
  ) || {
    exitcode=$?
    [[ $exitcode -eq 124 ]] || exit $exitcode
  }
  wc -l validator-sanity.log
  if grep -C100 panic validator-sanity.log; then
    echo "^^^ +++"
    echo "Panic observed"
    exit 1
  else
    echo "Validator sanity log looks ok"
  fi
else
  echo "^^^ +++"
  echo "Note: validator sanity disabled"
fi

echo --- Pass
