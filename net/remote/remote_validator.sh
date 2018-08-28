#!/bin/bash -e

[[ -n $FORCE ]] || exit

PATH="$HOME"/.cargo/bin:"$PATH"

rsync -vPrz "$1":~/.cargo/bin/solana* ~/.cargo/bin/

export USE_INSTALL=1
./multinode-demo/setup.sh
./multinode-demo/validator.sh "$1":~/solana "$1" >validator.log 2>&1 &
