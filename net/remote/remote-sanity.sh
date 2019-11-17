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
  solana_gossip=solana-gossip
  solana_install=solana-install
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

echo "+++ $sanityTargetIp: validators"
(
  set -x
  $solana_cli --url http://"$sanityTargetIp":8899 show-validators
)

echo "+++ $sanityTargetIp: node count ($numSanityNodes expected)"
(
  set -x

  nodeArg="num-nodes"
  if $rejectExtraNodes; then
    nodeArg="num-nodes-exactly"
  fi

  $solana_gossip spy --entrypoint "$sanityTargetIp:8001" \
    --$nodeArg "$numSanityNodes" --timeout 60 \
)

echo "--- $sanityTargetIp: RPC API: getTransactionCount"
(
  set -x
  curl --retry 5 --retry-delay 2 --retry-connrefused \
    -X POST -H 'Content-Type: application/json' \
    -d '{"jsonrpc":"2.0","id":1, "method":"getTransactionCount"}' \
    http://"$sanityTargetIp":8899
)

if [[ "$airdropsEnabled" = true ]]; then
  echo "--- $sanityTargetIp: wallet sanity"
  (
    set -x
    scripts/wallet-sanity.sh --url http://"$sanityTargetIp":8899
  )
else
  echo "^^^ +++"
  echo "Note: wallet sanity is disabled as airdrops are disabled"
fi

if $installCheck && [[ -r update_manifest_keypair.json ]]; then
  echo "--- $sanityTargetIp: solana-install test"

  (
    set -x
    rm -rf install-data-dir
    $solana_install init \
      --no-modify-path \
      --data-dir install-data-dir \
      --url http://"$sanityTargetIp":8899 \

    $solana_install info
  )
fi

echo --- Pass
