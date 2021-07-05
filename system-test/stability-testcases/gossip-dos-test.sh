#!/usr/bin/env bash

set -e
cd "$(dirname "$0")"
SAFECOIN_ROOT="$(cd ../..; pwd)"

logDir="$PWD"/logs
rm -rf "$logDir"
mkdir "$logDir"

solanaInstallDataDir=$PWD/releases
solanaInstallGlobalOpts=(
  --data-dir "$solanaInstallDataDir"
  --config "$solanaInstallDataDir"/config.yml
  --no-modify-path
)

# Install all the safecoin versions
bootstrapInstall() {
  declare v=$1
  if [[ ! -h $solanaInstallDataDir/active_release ]]; then
    sh "$SAFECOIN_ROOT"/install/safecoin-install-init.sh "$v" "${solanaInstallGlobalOpts[@]}"
  fi
  export PATH="$solanaInstallDataDir/active_release/bin/:$PATH"
}

bootstrapInstall "edge"
safecoin-install-init --version
safecoin-install-init edge
safecoin-gossip --version
safecoin-dos --version

killall safecoin-gossip || true
safecoin-gossip spy --gossip-port 10015 > "$logDir"/gossip.log 2>&1 &
solanaGossipPid=$!
echo "safecoin-gossip pid: $solanaGossipPid"
sleep 5
safecoin-dos --mode gossip --data-type random --data-size 1232 &
dosPid=$!
echo "safecoin-dos pid: $dosPid"

pass=true

SECONDS=
while ((SECONDS < 600)); do
  if ! kill -0 $solanaGossipPid; then
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

kill $solanaGossipPid || true
kill $dosPid || true
wait || true

$pass && echo Pass
