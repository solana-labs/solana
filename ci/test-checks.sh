#!/usr/bin/env bash

set -e

cd "$(dirname "$0")/.."

source ci/_
source ci/rust-version.sh stable
source ci/rust-version.sh nightly
eval "$(ci/channel-info.sh)"
cargo="$(readlink -f "./cargo")"

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

  "$cargo" stable --version --verbose
  "$cargo" nightly --version --verbose

  "$cargo" stable clippy --version --verbose
  "$cargo" nightly clippy --version --verbose

  # audit is done only with "$cargo stable"
  "$cargo" stable audit --version

  grcov --version
)

export RUST_BACKTRACE=1
export RUSTFLAGS="-D warnings -A incomplete_features"

# Only force up-to-date lock files on edge
if [[ $CI_BASE_BRANCH = "$EDGE_CHANNEL" ]]; then
  # Exclude --benches as it's not available in rust stable yet
  if _ scripts/cargo-for-all-lock-files.sh stable check --locked --tests --bins --examples; then
    true
  else
    check_status=$?
    echo "$0: Some Cargo.lock might be outdated; sync them (or just be a compilation error?)" >&2
    echo "$0: protip: $ ./scripts/cargo-for-all-lock-files.sh [--ignore-exit-code] ... \\" >&2
    echo "$0:   [tree (for outdated Cargo.lock sync)|check (for compilation error)|update -p foo --precise x.y.z (for your Cargo.toml update)] ..." >&2
    exit "$check_status"
  fi

   # Ensure nightly and --benches
  _ scripts/cargo-for-all-lock-files.sh nightly check --locked --all-targets
else
  echo "Note: cargo-for-all-lock-files.sh skipped because $CI_BASE_BRANCH != $EDGE_CHANNEL"
fi

 _ ci/order-crates-for-publishing.py

nightly_clippy_allows=()

# -Z... is needed because of clippy bug: https://github.com/rust-lang/rust-clippy/issues/4612
# run nightly clippy for `sdk/` as there's a moderate amount of nightly-only code there
 _ scripts/cargo-for-all-lock-files.sh -- nightly clippy -Zunstable-options --all-targets -- \
   --deny=warnings \
   --deny=clippy::integer_arithmetic \
   "${nightly_clippy_allows[@]}"

_ scripts/cargo-for-all-lock-files.sh -- nightly sort --workspace --check
_ scripts/cargo-for-all-lock-files.sh -- nightly fmt --all -- --check

 _ ci/do-audit.sh

echo --- ok
