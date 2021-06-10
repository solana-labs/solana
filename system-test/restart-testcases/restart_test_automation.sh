#!/usr/bin/env bash

set -ex

# shellcheck disable=SC1090
# shellcheck disable=SC1091
source "$(dirname "$0")"/../automation_utils.sh

RESULT_FILE="$1"

if [[ -z $CONSENSUS_TIMEOUT ]]; then
  CONSENSUS_TIMEOUT=180
fi

startGpuMode="off"
if [[ -z $ENABLE_GPU ]]; then
  ENABLE_GPU=false
fi
if [[ "$ENABLE_GPU" = "true" ]]; then
  startGpuMode="on"
fi

declare maybeAsyncNodeInit
if [[ "$ASYNC_NODE_INIT" = "true" ]]; then
  maybeAsyncNodeInit="--async-node-init"
fi

# Restart the network
"$REPO_ROOT"/net/net.sh stop

sleep 2

# shellcheck disable=SC2086
"$REPO_ROOT"/net/net.sh start --skip-setup --no-snapshot-fetch --no-deploy \
  --gpu-mode $startGpuMode $maybeAsyncNodeInit

# wait until consensus
start=$SECONDS
activeStake=$(get_active_stake)
while [[ $((SECONDS - start)) -lt $CONSENSUS_TIMEOUT ]]; do
  currentStake=$(get_current_stake)
  echo "$((SECONDS - start))s: Current stake $currentStake, Active stake $activeStake" >> "$RESULT_FILE"
  if [[ $activeStake -eq $currentStake ]]; then
    echo "Restart Test Succeeded" >>"$RESULT_FILE"
    exit 0
  fi
  sleep 5
done

echo "Could not establish consensus in $CONSENSUS_TIMEOUT seconds" >> "$RESULT_FILE"
exit 1
