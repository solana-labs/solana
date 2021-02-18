#!/usr/bin/env bash

so=$1
if [[ ! -r $so ]]; then
  echo "Error: file not found: $so"
  exit 1
fi
so_stripped=$2
if [[ -z $so_stripped ]]; then
  echo "Usage: $0 unstripped.so stripped.so"
  exit 1
fi

bpf_sdk=$(cd "$(dirname "$0")/.." && pwd)
# shellcheck source=sdk/bpf/env.sh
source "$bpf_sdk"/env.sh

set -e
out_dir=$(dirname "$so_stripped")
if [[ ! -d $out_dir ]]; then
  mkdir -p "$out_dir"
fi
"$bpf_sdk"/dependencies/bpf-tools/llvm/bin/llvm-objcopy --strip-all "$so" "$so_stripped"
