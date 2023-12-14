#!/usr/bin/env bash
# To prevent usange of `./cargo` without `nightly`
# Introduce cargoNighlty and disable warning to use word splitting
# shellcheck disable=SC2086

set -e

cd "$(dirname "$0")/.."

source ci/_
source ci/rust-version.sh stable
source ci/rust-version.sh nightly
eval "$(ci/channel-info.sh)"
cargoNightly="$(readlink -f "./cargo") nightly"

echo --- build environment
(
  set -x

  rustup run "$rust_stable" rustc --version --verbose
  rustup run "$rust_nightly" rustc --version --verbose

  cargo --version --verbose
  $cargoNightly --version --verbose

  cargo clippy --version --verbose
  $cargoNightly clippy --version --verbose

  $cargoNightly hack --version --verbose

  # audit is done only with "$cargo stable"
  cargo audit --version

  grcov --version

  sccache --version

  wasm-pack --version
)

export RUST_BACKTRACE=1
export RUSTFLAGS="-D warnings -A incomplete_features"

# run cargo check for all rust files in this monorepo for faster turnaround in
# case of any compilation/build error for nightly

# Only force up-to-date lock files on edge
if [[ $CI_BASE_BRANCH = "$EDGE_CHANNEL" ]]; then
  if _ scripts/cargo-for-all-lock-files.sh "+${rust_nightly}" check --locked --workspace --all-targets --features dummy-for-ci-check; then
    true
  else
    check_status=$?
    echo "$0: Some Cargo.lock might be outdated; sync them (or just be a compilation error?)" >&2
    echo "$0: protip: $ ./scripts/cargo-for-all-lock-files.sh [--ignore-exit-code] ... \\" >&2
    echo "$0:   [tree (for outdated Cargo.lock sync)|check (for compilation error)|update -p foo --precise x.y.z (for your Cargo.toml update)] ..." >&2
    exit "$check_status"
  fi
else
  echo "Note: cargo-for-all-lock-files.sh skipped because $CI_BASE_BRANCH != $EDGE_CHANNEL"
fi

_ ci/order-crates-for-publishing.py

_ scripts/cargo-clippy.sh

if [[ -n $CI ]]; then
  # exclude from printing "Checking xxx ..."
  _ scripts/cargo-for-all-lock-files.sh -- "+${rust_nightly}" sort --workspace --check > /dev/null
else
  _ scripts/cargo-for-all-lock-files.sh -- "+${rust_nightly}" sort --workspace --check
fi

_ scripts/check-dev-context-only-utils.sh tree

_ scripts/cargo-for-all-lock-files.sh -- "+${rust_nightly}" fmt --all -- --check

_ ci/do-audit.sh

if [[ -n $CI ]] && [[ $CHANNEL = "stable" ]]; then
  _ ci/check-install-all.sh
fi

echo --- ok
