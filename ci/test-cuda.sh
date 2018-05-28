#!/bin/bash -e

cd "$(dirname "$0")/.."

LIB=libcuda_verify_ed25519.a
if [[ ! -r $LIB ]]; then
  if [[ -z "${libcuda_verify_ed25519_URL:-}" ]]; then
    echo "$0 skipped.  Unable to locate $LIB"
    exit 0
  fi

  export LD_LIBRARY_PATH=/usr/local/cuda/lib64
  export PATH=$PATH:/usr/local/cuda/bin
  curl -X GET -o $LIB "$libcuda_verify_ed25519_URL"
fi

# shellcheck disable=SC1090    # <-- shellcheck can't follow ~
source ~/.cargo/env
cargo test --features=cuda

exit 0
