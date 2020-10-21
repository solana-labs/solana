#!/usr/bin/env bash

if [[ "$#" -ne 1 ]]; then
    echo "Error: Must provide the full path to the project to build"
    exit 1
fi
if [[ ! -f "$1/Cargo.toml" ]]; then
      echo "Error: Cannot find project: $1"
    exit 1
fi
bpf_sdk=$(cd "$(dirname "$0")/.." && pwd)

echo "Building $1"
set -e
# shellcheck source=sdk/bpf/env.sh
source "$bpf_sdk"/env.sh

cd "$1"
"$XARGO" build --target "$XARGO_TARGET" --release --no-default-features --features program

{ { set +x; } 2>/dev/null; echo Success; }
