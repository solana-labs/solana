#!/usr/bin/env bash

set -e
cd "$(dirname "$0")"
SOLANA_ROOT="$(cd ../..; pwd)"


if [[ $(uname) != Linux ]]; then
  echo Error: this test is Linux only
  exit 1
fi

logDir="$PWD"/logs
rm -rf "$logDir"
mkdir "$logDir"

solanaInstallDataDir=$PWD/releases
solanaInstallGlobalOpts=(
  --data-dir "$solanaInstallDataDir"
  --config "$solanaInstallDataDir"/config.yml
  --no-modify-path
)

# Install all the solana versions
bootstrapInstall() {
  declare v=$1
  if [[ ! -h $solanaInstallDataDir/active_release ]]; then
    sh "$SOLANA_ROOT"/install/solana-install-init.sh "$v" "${solanaInstallGlobalOpts[@]}"
  fi
  export PATH="$solanaInstallDataDir/active_release/bin/:$PATH"
}

bootstrapInstall "edge"

killall solana-gossip || true
solana-gossip spy --gossip-port 8001 > "$logDir"/gossip.log 2>&1 &
solanaGossipPid=$!
echo "solana-gossip pid: $solanaGossipPid"
sleep 5
dd if=/dev/zero bs=1232 > /dev/udp/127.0.0.1/8001 &
ddPid=$!
echo "dd pid: $ddPid"

pass=true

SECONDS=
while ((SECONDS < 600)); do
  if ! kill -0 $solanaGossipPid; then
    echo "solana-gossip is no longer running after $SECONDS seconds"
    pass=false
    break
  fi
  if ! kill -0 $ddPid; then
    echo "dd is no longer running after $SECONDS seconds"
    pass=false
    break
  fi
  sleep 1
done

kill $solanaGossipPid || true
kill $ddPid || true
wait || true

$pass && echo Pass
