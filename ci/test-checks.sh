#!/usr/bin/env bash
set -e

cd "$(dirname "$0")/.."

source ci/_
source ci/rust-version.sh stable
source ci/rust-version.sh nightly
eval "$(ci/channel-info.sh)"

export RUST_BACKTRACE=1
export RUSTFLAGS="-D warnings"

<<<<<<< HEAD
=======
# Only force up-to-date lock files on edge
if [[ $CI_BASE_BRANCH = "$EDGE_CHANNEL" ]]; then
  # Exclude --benches as it's not available in rust stable yet
  if _ scripts/cargo-for-all-lock-files.sh +"$rust_stable" check --locked --tests --bins --examples; then
    true
  else
    check_status=$?
    echo "Some Cargo.lock might be outdated; update them (or just be a compilation error?)"
    echo "protip: you can use ./scripts/cargo-for-all-lock-files.sh [check|update] ..."
    exit "$check_status"
  fi
else
  echo "Note: cargo-for-all-lock-files.sh skipped because $CI_BASE_BRANCH != $EDGE_CHANNEL"
fi

# Ensure nightly and --benches
_ scripts/cargo-for-all-lock-files.sh +"$rust_nightly" check --locked --all-targets

_ ci/order-crates-for-publishing.py
>>>>>>> a25ea8e77... Only force up-to-date lock files on edge
_ cargo +"$rust_stable" fmt --all -- --check

# Clippy gets stuck for unknown reasons if sdk-c is included in the build, so check it separately.
# See https://github.com/solana-labs/solana/issues/5503
_ cargo +"$rust_stable" clippy --version
_ cargo +"$rust_stable" clippy --all --exclude solana-sdk-c -- --deny=warnings
_ cargo +"$rust_stable" clippy --manifest-path sdk-c/Cargo.toml -- --deny=warnings

_ cargo +"$rust_stable" audit --version
_ cargo +"$rust_stable" audit --ignore RUSTSEC-2020-0002 --ignore RUSTSEC-2020-0008
_ ci/order-crates-for-publishing.py
_ docs/build.sh

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
