#!/bin/bash -e

set -o xtrace

cd $(dirname $0)/..

if [[ -z "$libgf_complete_URL" ]]; then
  echo libgf_complete_URL undefined
  exit 1
fi

if [[ -z "$libJerasure_URL" ]]; then
  echo libJerasure_URL undefined
  exit 1
fi

export LD_LIBRARY_PATH=/usr/local/cuda/lib64
export PATH=$PATH:/usr/local/cuda/bin
curl -X GET -o libJerasure.so "$libJerasure_URL"
curl -X GET -o libgf_complete.so "$libgf_complete"

source $HOME/.cargo/env
cargo test --features="erasure"

exit 0
