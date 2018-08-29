#!/bin/bash -e

cd "$(dirname "$0")"/../..
source net/common.sh
loadConfigFile

PATH="$HOME"/.cargo/bin:"$PATH"

./fetch-perf-libs.sh

./script/install-earlyoom.sh
./scripts/oom-monitor.sh  > oom-monitor.log 2>&1 &

export USE_INSTALL=1
export SOLANA_CUDA=1
./multinode-demo/setup.sh
./multinode-demo/drone.sh > drone.log 2>&1 &
./multinode-demo/leader.sh > leader.log 2>&1 &
