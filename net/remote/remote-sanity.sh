#!/usr/bin/env bash
set -e
#
# This script is to be run on the bootstrap full node
#

cd "$(dirname "$0")"/../..

sanityTargetIp="$1"
shift

deployMethod=
entrypointIp=
numNodes=
failOnValidatorBootupFailure=

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

[[ -n $sanityTargetIp ]] || missing sanityTargetIp
[[ -n $deployMethod ]]   || missing deployMethod
[[ -n $entrypointIp ]]   || missing entrypointIp
[[ -n $numNodes ]]       || missing numNodes
[[ -n $failOnValidatorBootupFailure ]] || missing failOnValidatorBootupFailure

ledgerVerify=true
validatorSanity=true
installCheck=true
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
  noInstallCheck)
    installCheck=false
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

  entrypointRsyncUrl="$sanityTargetIp:~/solana"

  solana_gossip=solana-gossip
  solana_install=solana-install
  solana_keygen=solana-keygen
  solana_ledger_tool=solana-ledger-tool

  ledger=config-local/bootstrap-leader-ledger
  client_id=config-local/client-id.json
  ;;
*)
  echo "Unknown deployment method: $deployMethod"
  exit 1
esac

if $failOnValidatorBootupFailure; then
  numSanityNodes="$numNodes"
else
  numSanityNodes=1
  if $rejectExtraNodes; then
    echo "rejectExtraNodes cannot be used with failOnValidatorBootupFailure"
    exit 1
  fi
fi

echo "+++ $sanityTargetIp: node count ($numSanityNodes expected)"
(
  set -x
  $solana_keygen new -f -o "$client_id"

  nodeArg="num-nodes"
  if $rejectExtraNodes; then
    nodeArg="num-nodes-exactly"
  fi

  timeout 2m $solana_gossip --entrypoint "$sanityTargetIp:8001" \
    spy --$nodeArg "$numSanityNodes" \
)

echo "--- $sanityTargetIp: RPC API: getTransactionCount"
(
  set -x
  curl --retry 5 --retry-delay 2 --retry-connrefused \
    -X POST -H 'Content-Type: application/json' \
    -d '{"jsonrpc":"2.0","id":1, "method":"getTransactionCount"}' \
    http://"$sanityTargetIp":8899
)

echo "--- $sanityTargetIp: wallet sanity"
(
  set -x
  scripts/wallet-sanity.sh --url http://"$sanityTargetIp":8899
)

echo "--- $sanityTargetIp: verify ledger"
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


echo "--- $sanityTargetIp: validator sanity"
if $validatorSanity; then
  (
    set -x -o pipefail
    timeout 10s ./multinode-demo/validator-x.sh --stake 0 \
      "$entrypointRsyncUrl" \
      "$sanityTargetIp:8001" 2>&1 | tee validator-sanity.log
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

if $installCheck && [[ -r update_manifest_keypair.json ]]; then
  echo "--- $sanityTargetIp: solana-install test"

  (
    set -x
    update_manifest_pubkey=$($solana_keygen pubkey update_manifest_keypair.json)
    rm -rf install-data-dir
    $solana_install init \
      --no-modify-path \
      --data-dir install-data-dir \
      --url http://"$sanityTargetIp":8899 \
      --pubkey "$update_manifest_pubkey"

    $solana_install info
  )
fi

echo --- Pass
