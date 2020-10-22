#!/usr/bin/env bash

bpf_sdk=$(cd "$(dirname "$0")/.." && pwd)
# shellcheck source=sdk/bpf/env.sh
source "$bpf_sdk"/env.sh
exec "$bpf_sdk"/dependencies/llvm-native/bin/llvm-objcopy "$@"
