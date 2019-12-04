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

_ cargo +"$rust_stable" audit --version
_ cargo +"$rust_stable" audit --ignore RUSTSEC-2019-0013 --ignore RUSTSEC-2018-0015
_ ci/nits.sh
_ ci/order-crates-for-publishing.py
_ book/build.sh
_ ci/check-ssh-keys.sh

{
  cd programs/bpf
  _ cargo +"$rust_stable" audit
  for project in rust/*/ ; do
    echo "+++ do_bpf_checks $project"
    (
      cd "$project"
      _ cargo +"$rust_stable" fmt -- --check
      _ cargo +"$rust_nightly" test
      _ cargo +"$rust_nightly" clippy --version
      _ cargo +"$rust_nightly" clippy -- --deny=warnings --allow=clippy::missing_safety_doc
    )
  done
}

echo --- ok
