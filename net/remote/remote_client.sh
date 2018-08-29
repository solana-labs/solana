#!/bin/bash -e

[[ -n $1 ]] || exit

cd "$(dirname "$0")"/../..
source net/common.sh
loadConfigFile

PATH="$HOME"/.cargo/bin:"$PATH"
rsync -vPrz "$1":~/.cargo/bin/solana* ~/.cargo/bin/

numNodes=1 # TODO: Pass this in

./script/install-earlyoom.sh
./scripts/oom-monitor.sh  > oom-monitor.log 2>&1 &

export USE_INSTALL=1
multinode-demo/client.sh "$1":~/solana $numNodes --loop -s 600 --sustained > client.log 2>&1 &
