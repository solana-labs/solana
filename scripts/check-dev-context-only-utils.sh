#!/usr/bin/env bash

set -eo pipefail
cd "$(dirname "$0")/.."
source ci/_
# only nightly is used uniformly as we contain good amount of nightly-only code
# (benches, frozen abi...)
source ci/rust-version.sh nightly

# There's a special common feature called `dev-context-only-utils` to
# overcome cargo's issue: https://github.com/rust-lang/cargo/issues/8379
# This feature is like `cfg(test)`, which works between crates.
#
# Unfortunately, this in turn needs some special checks to avoid common
# pitfalls of `dev-context-only-utils` itself.
#
# Firstly, detect any misuse of dev-context-only-utils as normal/build
# dependencies.  Also, allow some exceptions for special purpose crates. This
# white-listing mechanism can be used for core-development-oriented crates like
# bench bins.
#
# Put differently, use of dev-context-only-utils is forbidden for non-dev
# dependencies in general. However, allow its use for non-dev dependencies only
# if its use is confined under a dep. subgraph with all nodes being marked as
# dev-context-only-utils.

# Add your troubled package which seems to want to use `dev-context-only-utils`
# as normal (not dev) dependencies, only if you're sure that there's good
# reason to bend dev-context-only-utils's original intention and that listed
# package isn't part of released binaries.
declare tainted_packages=(
)

# convert to comma separeted (ref: https://stackoverflow.com/a/53839433)
printf -v allowed '"%s",' "${tainted_packages[@]}"
allowed="${allowed%,}"

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
      | map(select((.kind // "normal") == "normal"))
      | map({
        "crate" : \$crate,
        "dependency" : .name,
        "dependencyFeatures" : .features,
      })
    )
  )
  | flatten
  | map(select(
    (.dependencyFeatures
      | index("dev-context-only-utils")
    ) and (.crate as \$needle
      | ([$allowed] | index(\$needle))
      | not
    )
  ))
  | map([.crate, .dependency] | join(": "))
  | join("\n    ")
EOF
  )

  abusers="$(_ cargo "+${rust_nightly}" metadata --format-version=1 |
    jq -r "$query")"
  if [[ -n "$abusers" ]]; then
    cat <<EOF 1>&2
\`dev-context-only-utils\` must not be used as normal dependencies, but is by \
"([crate]: [dependency])":
    $abusers
EOF
    exit 1
  fi

  # Sanity-check that tainted packages has undergone the proper tedious rituals
  # to be justified as such.
  query=$(cat <<EOF
.packages
  | map([.name, (.features | keys)] as [\$this_crate, \$this_feature]
  | if .name as \$needle | ([$allowed] | index(\$needle))
  then
    {
      "crate": \$this_crate,
      "crateFeatures": \$this_feature,
    }
  elif .dependencies | any(
    .name as \$needle | ([$allowed] | index(\$needle))
  )
  then
    .dependencies
      | map({
        "crate": \$this_crate,
        "crateFeatures": \$this_feature,
      })
  else
    []
  end)
  | flatten
  | map(select(
    (.crateFeatures | index("dev-context-only-utils")) | not
  ))
  | map(.crate)
  | join("\n    ")
EOF
    )

    misconfigured_crates=$(
      _ cargo "+${rust_nightly}" metadata \
        --format-version=1 \
        | jq -r "$query"
    )
    if [[ -n "$misconfigured_crates" ]]; then
      cat <<EOF 1>&2
All crates marked \`tainted\`, as well as their dependents, MUST declare the \
\`dev-context-only-utils\`. The following crates are in violation:
    $misconfigured_crates
EOF
    exit 1
  fi
fi

# Detect possible compilation errors of problematic usage of
# `dev-context-only-utils`-gated code without being explicitly declared as such
# in respective workspace member `Cargo.toml`s. This cannot be detected with
# `--workspace --all-targets`, due to unintentional `dev-context-only-utils`
# feature activation by cargo's feature unification mechanism.  So, we use
# `cargo hack` to exhaustively build each individual workspace members in
# isolation to work around.
#
# 1. Check implicit usage of `dev-context-only-utils`-gated code in non-dev (=
# production) code by building without dev dependencies (= tests/benches) for
# each crate
# 2. Check implicit usage of `dev-context-only-utils`-gated code in dev (=
# test/benches) code by building in isolation from other crates, which might
# happen to enable `dev-context-only-utils`
if [[ $mode = "check-bins" || $mode = "full" ]]; then
  _ cargo "+${rust_nightly}" hack check --bins
fi
if [[ $mode = "check-all-targets" || $mode = "full" ]]; then
  _ cargo "+${rust_nightly}" hack check --all-targets
fi
