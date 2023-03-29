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

sbf_sdk=$(cd "$(dirname "$0")/.." && pwd)
# shellcheck source=sdk/sbf/env.sh
source "$sbf_sdk"/env.sh

set -e
out_dir=$(dirname "$so_stripped")
if [[ ! -d $out_dir ]]; then
  mkdir -p "$out_dir"
fi
"$sbf_sdk"/dependencies/platform-tools/llvm/bin/llvm-objcopy --strip-all "$so" "$so_stripped"
