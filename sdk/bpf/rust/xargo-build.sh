#!/usr/bin/env bash

bpf_sdk=$(cd "$(dirname "$0")/.." && pwd)
# shellcheck source=sdk/bpf/env.sh
source "$bpf_sdk"/env.sh

set -e
(
  while true; do
    if [[ -r Xargo.toml ]]; then
      break;
    fi
    if [[ $PWD = / ]]; then
      cat <<EOF
Error: Failed to find Xargo.toml

Please create a Xargo.toml file in the same directory as your Cargo.toml with
the following contents:

  [target.bpfel-unknown-unknown.dependencies.std]
  features = []

EOF
      exit 1
    fi
    cd ..
  done
)
exec "$XARGO" build --target "$XARGO_TARGET" --release "$@"
