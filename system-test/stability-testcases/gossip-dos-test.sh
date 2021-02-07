#!/usr/bin/env bash

set -e
cd "$(dirname "$0")"
SAFECOIN_ROOT="$(cd ../..; pwd)"

logDir="$PWD"/logs
rm -rf "$logDir"
mkdir "$logDir"

safecoinInstallDataDir=$PWD/releases
safecoinInstallGlobalOpts=(
  --data-dir "$safecoinInstallDataDir"
  --config "$safecoinInstallDataDir"/config.yml
  --no-modify-path
)

# Install all the safecoin versions
bootstrapInstall() {
  declare v=$1
  if [[ ! -h $safecoinInstallDataDir/active_release ]]; then
    sh "$SAFECOIN_ROOT"/install/safecoin-install-init.sh "$v" "${safecoinInstallGlobalOpts[@]}"
  fi
  export PATH="$safecoinInstallDataDir/active_release/bin/:$PATH"
}

bootstrapInstall "edge"
safecoin-install-init --version
safecoin-install-init edge
safecoin-gossip --version
safecoin-dos --version

killall safecoin-gossip || true
safecoin-gossip spy --gossip-port 8001 > "$logDir"/gossip.log 2>&1 &
safecoinGossipPid=$!
echo "safecoin-gossip pid: $safecoinGossipPid"
sleep 5
safecoin-dos --mode gossip --data-type random --data-size 1232 &
dosPid=$!
echo "safecoin-dos pid: $dosPid"

pass=true

SECONDS=
while ((SECONDS < 600)); do
  if ! kill -0 $safecoinGossipPid; then
    echo "safecoin-gossip is no longer running after $SECONDS seconds"
    pass=false
    break
  fi
  if ! kill -0 $dosPid; then
    echo "safecoin-dos is no longer running after $SECONDS seconds"
    pass=false
    break
  fi
  sleep 1
done

kill $safecoinGossipPid || true
kill $dosPid || true
wait || true

$pass && echo Pass
