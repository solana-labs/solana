#!/bin/bash -e

cd "$(dirname "$0")/.."

: "${libcuda_verify_ed25519_URL:?environment variable undefined}"

export LD_LIBRARY_PATH=/usr/local/cuda/lib64
export PATH=$PATH:/usr/local/cuda/bin
curl -X GET -o libcuda_verify_ed25519.a "$libcuda_verify_ed25519_URL"

# shellcheck disable=SC1090    # <-- shellcheck can't follow ~
source ~/.cargo/env
cargo test --features=cuda

exit 0
