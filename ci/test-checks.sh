#!/usr/bin/env bash

set -e

cd "$(dirname "$0")/.."

source ci/_
source ci/rust-version.sh stable
source ci/rust-version.sh nightly

echo --- build environment
(
  set -x

  rustup run "$rust_stable" rustc --version --verbose
  rustup run "$rust_nightly" rustc --version --verbose

  cargo +"$rust_stable" --version --verbose
  cargo +"$rust_nightly" --version --verbose

  cargo +"$rust_stable" clippy --version --verbose
  cargo +"$rust_nightly" clippy --version --verbose

  # audit is done only with stable
  cargo +"$rust_stable" audit --version
)

export RUST_BACKTRACE=1
export RUSTFLAGS="-D warnings"

if _ scripts/cargo-for-all-lock-files.sh +"$rust_nightly" check --locked --all-targets; then
  true
else
  check_status=$?
  echo "Some Cargo.lock is outdated; please update them as well"
  echo "protip: you can use ./scripts/cargo-for-all-lock-files.sh [check|update] ..."
  exit "$check_status"
fi

_ ci/order-crates-for-publishing.py
_ cargo +"$rust_stable" fmt --all -- --check

# -Z... is needed because of clippy bug: https://github.com/rust-lang/rust-clippy/issues/4612
_ cargo +"$rust_nightly" clippy -Zunstable-options --workspace --all-targets -- --deny=warnings

_ scripts/cargo-for-all-lock-files.sh +"$rust_stable" audit --ignore RUSTSEC-2020-0002 --ignore RUSTSEC-2020-0008

{
  cd programs/bpf
  _ cargo +"$rust_stable" audit
  for project in rust/*/ ; do
    echo "+++ do_bpf_checks $project"
    (
      cd "$project"
      _ cargo +"$rust_stable" fmt -- --check
      _ cargo +"$rust_nightly" test
      _ cargo +"$rust_nightly" clippy -- --deny=warnings --allow=clippy::missing_safety_doc
    )
  done
}

echo --- ok
