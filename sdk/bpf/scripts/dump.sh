#!/usr/bin/env bash

bpf_sdk=$(cd "$(dirname "$0")/.." && pwd)
# shellcheck source=sdk/bpf/env.sh
source "$bpf_sdk"/env.sh

so=$1
dump=$2
if [[ -z $so ]] || [[ -z $dump ]]; then
  echo "Usage: $0 bpf-program.so dump.txt"
  exit 1
fi

if [[ ! -r $so ]]; then
  echo "Error: File not found or readable: $so"
  exit 1
fi

if ! command -v rustfilt > /dev/null; then
  echo "Error: rustfilt not found.  It can be installed by running: cargo install rustfilt"
  exit 1
fi
if ! command -v readelf > /dev/null; then
  if [[ $(uname) = Darwin ]]; then
      echo "Error: readelf not found.  It can be installed by running: brew install binutils"
  else
      echo "Error: readelf not found."
  fi
  exit 1
fi

set -e
out_dir=$(dirname "$dump")
if [[ ! -d $out_dir ]]; then
  mkdir -p "$out_dir"
fi
dump_mangled=$dump.mangled

(
  set -ex
  ls -la "$so" > "$dump_mangled"
  readelf -aW "$so" >>"$dump_mangled"
  "$OBJDUMP" -print-imm-hex --source --disassemble "$so" >> "$dump_mangled"
  sed s/://g < "$dump_mangled" | rustfilt > "$dump"
)
rm -f "$dump_mangled"

if [[ ! -f "$dump" ]]; then
  echo "Error: Failed to create $dump"
  exit 1
fi

echo "Wrote $dump"
