#!/usr/bin/env bash
set -eo pipefail

here="$(dirname "$0")"

#shellcheck source=ci/bench/common.sh
source "$here"/common.sh

# Run core benches
_ cargo +"$rust_nightly" bench --manifest-path core/Cargo.toml ${V:+--verbose} \
  -- -Z unstable-options --format=json | tee -a "$BENCH_FILE"

# Run gossip benches
_ cargo +"$rust_nightly" bench --manifest-path gossip/Cargo.toml ${V:+--verbose} \
  -- -Z unstable-options --format=json | tee -a "$BENCH_FILE"

# Run poh benches
_ cargo +"$rust_nightly" bench --manifest-path poh/Cargo.toml ${V:+--verbose} \
  -- -Z unstable-options --format=json | tee -a "$BENCH_FILE"

# Run turbine benches
_ cargo +"$rust_nightly" bench --manifest-path turbine/Cargo.toml ${V:+--verbose} \
  -- -Z unstable-options --format=json | tee -a "$BENCH_FILE"
