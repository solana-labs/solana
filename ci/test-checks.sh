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

scripts/increment-cargo-version.sh check

# Disallow uncommitted Cargo.lock changes
(
  _ scripts/cargo-for-all-lock-files.sh tree >/dev/null
  set +e
  if ! _ git diff --exit-code; then
    cat <<EOF 1>&2

Error: Uncommitted Cargo.lock changes.
Run './scripts/cargo-for-all-lock-files.sh tree' and commit the result.
EOF
    exit 1
  fi
)

echo --- build environment
(
  set -x

  rustup run "$rust_stable" rustc --version --verbose
  rustup run "$rust_nightly" rustc --version --verbose

  cargo --version --verbose
  $cargoNightly --version --verbose

  cargo clippy --version --verbose
  $cargoNightly clippy --version --verbose

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

nightly_clippy_allows=(--allow=clippy::redundant_clone)

# Use nightly clippy, as frozen-abi proc-macro generates a lot of code across
# various crates in this whole monorepo (frozen-abi is enabled only under nightly
# due to the use of unstable rust feature). Likewise, frozen-abi(-macro) crates'
# unit tests are only compiled under nightly.
# Similarly, nightly is desired to run clippy over all of bench files because
# the bench itself isn't stabilized yet...
#   ref: https://github.com/rust-lang/rust/issues/66287
_ scripts/cargo-for-all-lock-files.sh -- "+${rust_nightly}" clippy --workspace --all-targets --features dummy-for-ci-check -- \
  --deny=warnings \
  --deny=clippy::default_trait_access \
  --deny=clippy::integer_arithmetic \
  --deny=clippy::used_underscore_binding \
  "${nightly_clippy_allows[@]}"

# temporarily run stable clippy as well to scan the codebase for
# `redundant_clone`s, which is disabled as nightly clippy is buggy:
#   https://github.com/solana-labs/solana/issues/31834
#
# can't use --all-targets:
#   error[E0554]: `#![feature]` may not be used on the stable release channel
_ scripts/cargo-for-all-lock-files.sh -- clippy --workspace  --tests --bins --examples --features dummy-for-ci-check -- \
  --deny=warnings \
  --deny=clippy::default_trait_access \
  --deny=clippy::integer_arithmetic \
  --deny=clippy::used_underscore_binding

if [[ -n $CI ]]; then
  # exclude from printing "Checking xxx ..."
  _ scripts/cargo-for-all-lock-files.sh -- "+${rust_nightly}" sort --workspace --check > /dev/null
else
  _ scripts/cargo-for-all-lock-files.sh -- "+${rust_nightly}" sort --workspace --check
fi

_ scripts/cargo-for-all-lock-files.sh -- "+${rust_nightly}" fmt --all -- --check

_ ci/do-audit.sh

echo --- ok
