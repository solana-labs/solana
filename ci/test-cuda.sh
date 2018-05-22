#!/bin/bash -e

cd $(dirname $0)/..

if [[ -z "$libcuda_verify_ed25519_URL" ]]; then
  echo libcuda_verify_ed25519_URL undefined
  exit 1
fi

export LD_LIBRARY_PATH=/usr/local/cuda/lib64
export PATH=$PATH:/usr/local/cuda/bin
curl -X GET -o libcuda_verify_ed25519.a "$libcuda_verify_ed25519_URL"

source $HOME/.cargo/env
cargo test --features=cuda

exit 0
