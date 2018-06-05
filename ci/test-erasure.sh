#!/bin/bash -e

set -o xtrace

cd "$(dirname "$0")/.."

if [[ -z "${libgf_complete_URL:-}" ]]; then
  echo libgf_complete_URL undefined
  exit 1
fi

if [[ -z "${libJerasure_URL:-}" ]]; then
  echo libJerasure_URL undefined
  exit 1
fi

curl -X GET -o libJerasure.so "$libJerasure_URL"
curl -X GET -o libgf_complete.so "$libgf_complete_URL"

ln -s libJerasure.so libJerasure.so.2
ln -s libJerasure.so libJerasure.so.2.0.0
ln -s libgf_complete.so libgf_complete.so.1.0.0
export LD_LIBRARY_PATH=$PWD:$LD_LIBRARY_PATH

# shellcheck disable=SC1090    # <-- shellcheck can't follow ~
source ~/.cargo/env
cargo test --features="erasure"

exit 0
