#!/bin/bash -e

[[ -n $FORCE ]] || exit

chmod 600 ~/.ssh/authorized_keys ~/.ssh/id_rsa

PATH="$HOME"/.cargo/bin:"$PATH"

touch ~/.ssh/known_hosts
ssh-keygen -R "$1" 2>/dev/null
ssh-keyscan "$1" >>~/.ssh/known_hosts 2>/dev/null

rsync -vPrz "$1":~/.cargo/bin/solana* ~/.cargo/bin/

# Run setup
USE_INSTALL=1 ./multinode-demo/setup.sh -p
USE_INSTALL=1 ./multinode-demo/validator.sh "$1":~/solana "$1" >validator.log 2>&1
