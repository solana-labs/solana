#!/usr/bin/env bash
set -e

cd "$(dirname "$0")/.."

./fetch-perf-libs.sh
# shellcheck source=/dev/null
source ./target/perf-libs/env.sh

FEATURES=bpf_c,cuda,erasure,chacha
exec ci/test-stable.sh "$FEATURES"
