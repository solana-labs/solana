#!/usr/bin/env bash

sbf_sdk=$(cd "$(dirname "$0")/.." && pwd)
# shellcheck source=sdk/sbf/env.sh
source "$sbf_sdk"/env.sh
exec "$sbf_sdk"/dependencies/platform-tools/llvm/bin/llvm-objcopy "$@"
