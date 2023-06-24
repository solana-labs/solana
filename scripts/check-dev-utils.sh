#!/usr/bin/env bash

set -eo pipefail
cd "$(dirname "$0")/.."
source ci/_
# only nightly is used uniformly as we contain good amount of nightly-only code
# (benches, frozen abi...)
source ci/rust-version.sh nightly

# There's a special common feature called `dev-utils` to
# overcome cargo's issue: https://github.com/rust-lang/cargo/issues/8379
# This feature is like `cfg(test)`, which works between crates.
#
# Unfortunately, this in turn needs some special checks to avoid common
# pitfalls of `dev-utils` itself.
#
# Firstly, detect any misuse of dev-utils as normal/build dependencies.
# Also, allow some exceptions for special purpose crates. This white-listing
# mechanism can be used for core-development-oriented crates like bench bins.
#
# Put differently, use of dev-utils is forbidden for non-dev dependencies in
# general. However, allow its use for non-dev dependencies only if its use
# is confined under a dep. subgraph with all nodes being marked as dev-utils.

# Add your troubled package which seems to want to use `dev-utils` as normal (not
# dev) dependencies, only if you're sure that there's good reason to bend
# dev-util's original intention and that listed package isn't part of released
# binaries.
# Note also that dev-utils-ci-marker feature must be added and all of its
# dependencies should be edited likewise if any.
declare dev_util_tainted_packages=(
)

mode=${1:-full}
if [[ $mode != "tree" &&
      $mode != "check-bins" &&
      $mode != "check-all-targets" &&
      $mode != "full" ]]; then
  echo "$0: unrecognized mode: $mode";
  exit 1
fi

if [[ $mode = "tree" || $mode = "full" ]]; then
  # Run against the entire workspace dep graph (sans $dev_util_tainted_packages)
  dev_utils_excludes=$(for tainted in "${dev_util_tainted_packages[@]}"; do
    echo "--exclude $tainted"
  done)
  # shellcheck disable=SC2086 # Don't want to double quote $dev_utils_excludes
  _ cargo "+${rust_nightly}" tree --workspace -f "{p} {f}" --edges normal,build \
    $dev_utils_excludes | (
    if grep -E -C 3 -m 10 "[, ]dev-utils([, ]|$)"; then
      echo "$0: dev-utils must not be used as normal dependencies" > /dev/stderr
      exit 1
    fi
  )

  # Sanity-check that tainted packages has undergone the proper tedious rituals
  # to be justified as such.
  for tainted in "${dev_util_tainted_packages[@]}"; do
    # dev-utils-ci-marker is special proxy feature needed only when using
    # dev-utils code as part of normal dependency. dev-utils will be enabled
    # indirectly via this feature only if prepared correctly
    _ cargo "+${rust_nightly}" tree --workspace -f "{p} {f}" --edges normal,build \
      --invert "$tainted" --features dev-utils-ci-marker | (
      if grep -E -C 3 -m 10 -v "[, ]dev-utils([, ]|$)"; then
        echo "$0: $tainted: All inverted dependencies must be with dev-utils" \
          > /dev/stderr
        exit 1
      fi
    )
  done
fi

# Detect possible compilation errors of problematic usage of `dev-utils`-gated code
# without being explicitly declared as such in respective workspace member
# `Cargo.toml`s. This cannot be detected with `--workspace --all-targets`, due
# to unintentional `dev-utils` feature activation by cargo's feature
# unification mechanism.
# So, we use `cargo hack` to exhaustively build each individual workspace
# members in isolation to work around.
#
# 1. Check implicit usage of `dev-utils`-gated code in non-dev (= production) code by
# building without dev dependencies (= tests/benches) for each crate
# 2. Check implicit usage of `dev-utils`-gated code in dev (= test/benches) code by
# building in isolation from other crates, which might happen to enable `dev-utils`
if [[ $mode = "check-bins" || $mode = "full" ]]; then
  _ cargo "+${rust_nightly}" hack check --bins
fi
if [[ $mode = "check-all-targets" || $mode = "full" ]]; then
  _ cargo "+${rust_nightly}" hack check --all-targets
fi
