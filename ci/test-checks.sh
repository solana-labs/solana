#!/usr/bin/env bash

set -e

cd "$(dirname "$0")/.."

source ci/_
source ci/rust-version.sh stable
source ci/rust-version.sh nightly
eval "$(ci/channel-info.sh)"

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
export RUSTFLAGS="-D warnings -A incomplete_features"

# Only force up-to-date lock files on edge
if [[ $CI_BASE_BRANCH = "$EDGE_CHANNEL" ]]; then
  # Exclude --benches as it's not available in rust stable yet
  if _ scripts/cargo-for-all-lock-files.sh +"$rust_stable" check --locked --tests --bins --examples; then
    true
  else
    check_status=$?
    echo "$0: Some Cargo.lock might be outdated; sync them (or just be a compilation error?)" >&2
    echo "$0: protip: $ ./scripts/cargo-for-all-lock-files.sh [--ignore-exit-code] ... \\" >&2
    echo "$0:   [tree (for outdated Cargo.lock sync)|check (for compilation error)|update -p foo --precise x.y.z (for your Cargo.toml update)] ..." >&2
    exit "$check_status"
  fi

  # Ensure nightly and --benches
  _ scripts/cargo-for-all-lock-files.sh +"$rust_nightly" check --locked --all-targets
else
  echo "Note: cargo-for-all-lock-files.sh skipped because $CI_BASE_BRANCH != $EDGE_CHANNEL"
fi

_ ci/order-crates-for-publishing.py
_ cargo +"$rust_stable" fmt --all -- --check

# -Z... is needed because of clippy bug: https://github.com/rust-lang/rust-clippy/issues/4612
# run nightly clippy for `sdk/` as there's a moderate amount of nightly-only code there
_ cargo +"$rust_nightly" clippy \
  -Zunstable-options --workspace --all-targets \
  -- --deny=warnings --allow=clippy::stable_sort_primitive

cargo_audit_ignores=(
  # failure is officially deprecated/unmaintained
  #
  # Blocked on multiple upstream crates removing their `failure` dependency.
  --ignore RUSTSEC-2020-0036

  # `net2` crate has been deprecated; use `socket2` instead
  #
  # Blocked on https://github.com/paritytech/jsonrpc/issues/575
  --ignore RUSTSEC-2020-0016
)
_ scripts/cargo-for-all-lock-files.sh +"$rust_stable" audit "${cargo_audit_ignores[@]}"

{
  cd programs/bpf
  _ cargo +"$rust_stable" audit
  for project in rust/*/ ; do
    echo "+++ do_bpf_checks $project"
    (
      cd "$project"
      _ cargo +"$rust_stable" fmt -- --check
      _ cargo +"$rust_nightly" test
      _ cargo +"$rust_nightly" clippy -- --deny=warnings \
        --allow=clippy::missing_safety_doc \
        --allow=clippy::stable_sort_primitive
    )
  done
}

echo --- ok
