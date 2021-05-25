#!/usr/bin/env bash

set -ex

# shellcheck disable=SC1090
# shellcheck disable=SC1091
source "$(dirname "$0")"/../automation_utils.sh

RESULT_FILE="$1"

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

declare maybeExtraPrimordialStakes
if [[ -n "$EXTRA_PRIMORDIAL_STAKES" ]]; then
  maybeExtraPrimordialStakes="--extra-primordial-stakes $EXTRA_PRIMORDIAL_STAKES"
fi

# Restart the network
"$REPO_ROOT"/net/net.sh stop

sleep 2

# shellcheck disable=SC2086
"$REPO_ROOT"/net/net.sh start --skip-setup --no-snapshot-fetch --no-deploy \
  --gpu-mode $startGpuMode $maybeAsyncNodeInit $maybeExtraPrimordialStakes

# TODO add the test here

echo "Restart Test Succeeded" >>"$RESULT_FILE"
