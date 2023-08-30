#!/usr/bin/env bash
set -eo pipefail

here="$(dirname "$0")"

#shellcheck source=ci/bench/common.sh
source "$here"/common.sh

# Run sdk benches
_ cargo +"$rust_nightly" bench --manifest-path sdk/Cargo.toml ${V:+--verbose} \
  -- -Z unstable-options --format=json | tee -a "$BENCH_FILE"

# Run runtime benches
_ cargo +"$rust_nightly" bench --manifest-path runtime/Cargo.toml ${V:+--verbose} \
  -- -Z unstable-options --format=json | tee -a "$BENCH_FILE"

(
  # solana-keygen required when building C programs
  _ cargo build --manifest-path=keygen/Cargo.toml
  export PATH="$PWD/target/debug":$PATH

  # Run sbf benches
  _ cargo +"$rust_nightly" bench --manifest-path programs/sbf/Cargo.toml ${V:+--verbose} --features=sbf_c \
    -- -Z unstable-options --format=json --nocapture | tee -a "$BENCH_FILE"
)

# Run banking/accounts bench. Doesn't require nightly, but use since it is already built.
_ cargo +"$rust_nightly" run --release --manifest-path banking-bench/Cargo.toml ${V:+--verbose} | tee -a "$BENCH_FILE"
_ cargo +"$rust_nightly" run --release --manifest-path accounts-bench/Cargo.toml ${V:+--verbose} -- --num_accounts 10000 --num_slots 4 | tee -a "$BENCH_FILE"

# Run zk-token-proof benches.
_ cargo +"$rust_nightly" bench --manifest-path programs/zk-token-proof/Cargo.toml ${V:+--verbose} | tee -a "$BENCH_FILE"
