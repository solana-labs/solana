#!/usr/bin/env bash

bpf_sdk=$(cd "$(dirname "$0")/.." && pwd)
# shellcheck source=sdk/bpf/env.sh
source "$bpf_sdk"/env.sh
exec "$bpf_sdk"/dependencies/bpf-tools/llvm/bin/llvm-objcopy "$@"
