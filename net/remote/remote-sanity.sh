#!/usr/bin/env bash
set -e
#
# This script is to be run on the bootstrap validator
#

cd "$(dirname "$0")"/../..

sanityTargetIp="$1"
shift

deployMethod=
entrypointIp=
numNodes=
failOnValidatorBootupFailure=
airdropsEnabled=true

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

installCheck=true
rejectExtraNodes=false
while [[ $1 = -o ]]; do
  opt="$2"
  shift 2
  case $opt in
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

if [[ -n $1 ]]; then
  export RUST_LOG="$1"
fi

source net/common.sh
loadConfigFile

case $deployMethod in
local|tar|skip)
  PATH="$HOME"/.cargo/bin:"$PATH"
  export USE_INSTALL=1
  solana_cli=solana
  safecoin_gossip=safecoin-gossip
  safecoin_install=safecoin-install
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

echo "--- $sanityTargetIp: validators"
(
  set -x
  $solana_cli --url http://"$sanityTargetIp":8328 validators
)

echo "--- $sanityTargetIp: node count ($numSanityNodes expected)"
(
  set -x

  nodeArg="num-nodes"
  if $rejectExtraNodes; then
    nodeArg="num-nodes-exactly"
  fi

  $safecoin_gossip spy --entrypoint "$sanityTargetIp:10015" \
    --$nodeArg "$numSanityNodes" --timeout 60 \
)

echo "--- $sanityTargetIp: RPC API: getTransactionCount"
(
  set -x
  curl --retry 5 --retry-delay 2 --retry-connrefused \
    -X POST -H 'Content-Type: application/json' \
    -d '{"jsonrpc":"2.0","id":1, "method":"getTransactionCount"}' \
    http://"$sanityTargetIp":8328
)

if [[ "$airdropsEnabled" = true ]]; then
  echo "--- $sanityTargetIp: wallet sanity"
  (
    set -x
    scripts/wallet-sanity.sh --url http://"$sanityTargetIp":8328
  )
else
  echo "^^^ +++"
  echo "Note: wallet sanity is disabled as airdrops are disabled"
fi

if $installCheck && [[ -r update_manifest_keypair.json ]]; then
  echo "--- $sanityTargetIp: safecoin-install test"

  (
    set -x
    rm -rf install-data-dir
    $safecoin_install init \
      --no-modify-path \
      --data-dir install-data-dir \
      --url http://"$sanityTargetIp":8328 \

    $safecoin_install info
  )
fi

echo --- Pass
