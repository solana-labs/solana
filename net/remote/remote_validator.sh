#!/bin/bash -e

[[ -n $1 ]] || exit

cd "$(dirname "$0")"/../..
source net/common.sh
loadConfigFile

PATH="$HOME"/.cargo/bin:"$PATH"

rsync -vPrz "$1":~/.cargo/bin/solana* ~/.cargo/bin/

export USE_INSTALL=1
./multinode-demo/setup.sh
./multinode-demo/validator.sh "$1":~/solana "$1" >validator.log 2>&1 &
