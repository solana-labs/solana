#!/usr/bin/env bash
set -e

cd "$(dirname "$0")/.."

usage() {
  exitcode=0
  if [[ -n "$1" ]]; then
    exitcode=1
    echo "Error: $*"
  fi
  cat <<EOF
usage: $0 [name] [cloud] [zone]

Sanity check a CD testnet

  name  - name of the network
  cloud - cloud provider to use (gce, ec2)
  zone  - cloud provider zone of the network

  Note: the SOLANA_METRICS_CONFIG environment variable is used to configure
        metrics
EOF
  exit $exitcode
}

netName=$1
cloudProvider=$2
zone=$3
[[ -n $netName ]] || usage ""
[[ -n $cloudProvider ]] || usage "Cloud provider not specified"
[[ -n $zone ]] || usage "Zone not specified"

set -x
echo "--- $cloudProvider.sh config"
net/"$cloudProvider".sh config -p "$netName" -z "$zone"
net/init-metrics.sh -e
echo --- net.sh sanity
net/net.sh sanity \
  ${NO_LEDGER_VERIFY:+-o noLedgerVerify} \
  ${NO_VALIDATOR_SANITY:+-o noValidatorSanity} \
  ${REJECT_EXTRA_NODES:+-o rejectExtraNodes} \

exit 0
