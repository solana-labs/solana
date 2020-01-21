#!/usr/bin/env bash
set -e

cd "$(dirname "$0")/.."
source ci/upload-ci-artifact.sh

usage() {
  exitcode=0
  if [[ -n "$1" ]]; then
    exitcode=1
    echo "Error: $*"
  fi
  cat <<EOF
usage: $0 [name] [cloud] [zone1] ... [zoneN]

Sanity check a testnet

  name            - name of the network
  cloud           - cloud provider to use (gce, ec2)
  zone1 .. zoneN  - cloud provider zones to check

  Note: the SOLANA_METRICS_CONFIG environment variable is used to configure
        metrics
EOF
  exit $exitcode
}

netName=$1
cloudProvider=$2
[[ -n $netName ]] || usage ""
[[ -n $cloudProvider ]] || usage "Cloud provider not specified"
shift 2

maybePublicNetwork=
if [[ $1 = -P ]]; then
  maybePublicNetwork=-P
  shift
fi
[[ -n $1 ]] || usage "zone1 not specified"

shutdown() {
  exitcode=$?

  set +e
  if [[ -d net/log ]]; then
    mv net/log net/log-sanity
    for logfile in net/log-sanity/*; do
      if [[ -f $logfile ]]; then
        upload-ci-artifact "$logfile"
        tail "$logfile"
      fi
    done
  fi
  exit $exitcode
}
rm -rf net/{log,-sanity}
rm -f net/config/config
trap shutdown EXIT INT

set -x
for zone in "$@"; do
  echo "--- $cloudProvider config [$zone]"
  timeout 5m net/"$cloudProvider".sh config $maybePublicNetwork -n 1 -p "$netName" -z "$zone"
  net/init-metrics.sh -e
  echo "+++ $cloudProvider.sh info"
  net/"$cloudProvider".sh info
  echo "--- net.sh sanity [$cloudProvider:$zone]"
  ok=true
  timeout 5m net/net.sh sanity \
    ${REJECT_EXTRA_NODES:+-o rejectExtraNodes} \
    ${NO_INSTALL_CHECK:+-o noInstallCheck} \
    $zone || ok=false

  if ! $ok; then
    net/net.sh logs
  fi
  $ok
done
