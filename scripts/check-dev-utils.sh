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
declare -r dev_utils_feature="dev-utils"
declare dev_util_tainted_packages=(
)

tainted=("${dev_util_tainted_packages[@]}")
if [[ ${#tainted[@]} -gt 0 ]]; then
  allowed="\"${tainted[0]}\""
  for package in "${tainted[@]:1}"; do
    allowed="${allowed},\"$package\""
  done
fi

mode=${1:-full}
case "$mode" in
  tree | check-bins | check-all-targets | full)
    ;;
  *)
    echo "$0: unrecognized mode: $mode";
    exit 1
    ;;
esac

if [[ $mode = "tree" || $mode = "full" ]]; then
  query=$(cat <<EOF
.packages
  | map(.name as \$crate
    | (.dependencies
      | map({
        "crate" : \$crate,
        "dependency" : .name,
        "dependencyFeatures" : .features
      })
    )
  )
  | flatten
  | map(select(
    (.dependencyFeatures
      | index("${dev_utils_feature}")
    ) and (.crate as \$needle
      | ([$allowed] | index(\$needle))
      | not
    )
  ))
  | map([.crate, .dependency] | join(": "))
  | join("\n      ")
EOF
)

  abusers="$(_ cargo "+${rust_nightly}" metadata --format-version=1 | jq -r "$query")"
  if [[ -n "$abusers" ]]; then
    cat <<EOF 1>&2
    ${dev_utils_feature} must not be used as normal dependencies, but is by: \`[crate]: [dependency]\`
      $abusers
EOF
  fi

  # Sanity-check that tainted packages has undergone the proper tedious rituals
  # to be justified as such.
  query=$(cat <<EOF
.packages
  | map(select(
    .dependencies
      | any(.name as \$needle
        | ([$allowed] | index(\$needle))
      )
  ))
  | map([.name, (.features | keys)] as [\$dependant, \$dependant_features]
    | (.dependencies
      | map({
        "crate" : .name,
        "crateFeatures" : .features,
        "dependant" : \$dependant,
        "dependantFeatures" : \$dependant_features
      })
    )
  )
  | flatten
  | map(select(
    ((.crateFeatures
      | index("${dev_utils_feature}"))
      | not
    ) or ((.dependantFeatures
      | index("${dev_utils_feature}"))
      | not
    )
  ))
  | map([.crate, .dependant] | join(": "))
  | join("\n      ")
EOF
)

  # dev-utils-ci-marker is special proxy feature needed only when using
  # dev-utils code as part of normal dependency. dev-utils will be enabled
  # indirectly via this feature only if prepared correctly
  misconfigured_crates=$(_ cargo "+${rust_nightly}" metadata --format-version=1 --features dev-utils-ci-marker | jq -r "$query")
  if [[ -n "$misconfigured_crates" ]]; then
    cat <<EOF 1>&2
    All crates marked \`tainted\`, as well as their dependents, MUST declare the
    \`$dev_utils_feature\`. The following crates are in violation. \`[crate]: [dependant]\`
      $misconfigured_crates
EOF
  fi
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
