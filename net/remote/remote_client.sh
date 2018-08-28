#!/bin/bash -e

[[ -n $FORCE ]] || exit

PATH="$HOME"/.cargo/bin:"$PATH"

rsync -vPrz "$1":~/.cargo/bin/solana* ~/.cargo/bin/

numNodes=1 # TODO: Pass this in
export USE_INSTALL=1

./multinode-demo/client.sh "$1":~/solana $numNodes --loop -s 600 --sustained >client.log 2>&1 &
