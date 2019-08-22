#!/usr/bin/env bash
set -e

cd "$(dirname "$0")/.."

source ci/_
source ci/rust-version.sh stable
source ci/rust-version.sh nightly

export RUST_BACKTRACE=1
export RUSTFLAGS="-D warnings"

_ cargo +"$rust_stable" fmt --all -- --check

# Clippy gets stuck for unknown reasons if sdk-c is included in the build, so check it separately.
# See https://github.com/solana-labs/solana/issues/5503
_ cargo +"$rust_stable" clippy --version
_ cargo +"$rust_stable" clippy --all --exclude solana-sdk-c -- --deny=warnings
_ cargo +"$rust_stable" clippy --manifest-path sdk-c/Cargo.toml -- --deny=warnings

# _ cargo +"$rust_stable" audit --version ### cargo-audit stopped supporting --version??  https://github.com/RustSec/cargo-audit/issues/100
_ cargo +"$rust_stable" audit
_ ci/nits.sh
_ ci/order-crates-for-publishing.py
_ book/build.sh

for project in sdk/bpf/rust/{rust-no-std,rust-utils,rust-test} programs/bpf/rust/*/ ; do
  echo "+++ do_bpf_check $project"
  (
    cd "$project"
    _ cargo +"$rust_stable" fmt --all -- --check
    _ cargo +"$rust_nightly" test --all
    _ cargo +"$rust_nightly" clippy --version
    _ cargo +"$rust_nightly" clippy --all -- --deny=warnings
    _ cargo +"$rust_stable" audit
  )
done

echo --- ok
