#!/bin/bash -e

deployMethod="$1"
netEntrypoint="$2"
numNodes="$3"

[[ -n $deployMethod ]] || exit
[[ -n $netEntrypoint ]] || exit
[[ -n $numNodes ]] || exit

shift 3

ledgerVerify=true
validatorSanity=true
while [[ $1 = "-o" ]]; do
  opt="$2"
  shift 2
  case $opt in
  noLedgerVerify)
    ledgerVerify=false
    ;;
  noValidatorSanity)
    validatorSanity=false
    ;;
  *)
    echo "Error: unknown option: $opt"
    exit 1
    ;;
  esac
done


cd "$(dirname "$0")"/../..
source net/common.sh
loadConfigFile

case $deployMethod in
snap)
  export USE_SNAP=1
  solana_bench_tps=/snap/bin/solana.bench-tps
  solana_ledger_tool=/snap/bin/solana.ledger-tool
  ledger=/var/snap/solana/current/config/ledger
  ;;
local)
  PATH="$HOME"/.cargo/bin:"$PATH"
  export USE_INSTALL=1

  solana_bench_tps=multinode-demo/client.sh
  solana_ledger_tool=solana-ledger-tool
  ledger=config/ledger
  netEntrypoint="$:~/solana"
  ;;
*)
  echo "Unknown deployment method: $deployMethod"
  exit 1
esac


echo "--- $netEntrypoint: wallet sanity"
(
  set -x
  multinode-demo/test/wallet-sanity.sh "$netEntrypoint"
)

echo "--- $netEntrypoint: node count"
(
  set -x
  $solana_bench_tps "$netEntrypoint" "$numNodes" -c
)

echo "--- $netEntrypoint: verify ledger"
if $ledgerVerify; then
  if [[ -d $ledger ]]; then
    (
      set -x
      rm -rf /var/tmp/ledger-verify
      cp -r $ledger /var/tmp/ledger-verify
      $solana_ledger_tool --ledger /var/tmp/ledger-verify verify
    )
  else
    echo "^^^ +++"
    echo "Ledger verify skipped"
  fi
else
  echo "^^^ +++"
  echo "Ledger verify skipped (NO_LEDGER_VERIFY defined)"
fi


echo "--- $netEntrypoint: validator sanity"
if $validatorSanity; then
  (
    ./multinode-demo/setup.sh -t validator
    set -e pipefail
    timeout 10s ./multinode-demo/validator.sh "$netEntrypoint" 2>&1 | tee validator.log
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

