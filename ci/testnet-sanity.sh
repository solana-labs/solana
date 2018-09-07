#!/bin/bash -e

cd "$(dirname "$0")/.."

usage() {
  exitcode=0
  if [[ -n "$1" ]]; then
    exitcode=1
    echo "Error: $*"
  fi
  cat <<EOF
usage: $0 [name]

Sanity check a CD testnet

  name  - name of the network

  Note: the SOLANA_METRICS_CONFIG environment variable is used to configure
        metrics
EOF
  exit $exitcode
}

netName=$1
[[ -n $netName ]] || usage ""

set -x
echo --- gce.sh config
net/gce.sh config -p "$netName"
net/init-metrics.sh -e
echo --- net.sh sanity
net/net.sh sanity ${NO_LEDGER_VERIFY:+-o noLedgerVerify}

exit 0
