#!/usr/bin/env bash

# Runs `cargo clippy` in all individual workspaces in the repository.
#
# We have a number of clippy parameters that we want to enforce across the
# code base.  They are defined here.
#
# This script is run by the CI, so if you want to replicate what the CI is
# doing, better run this script, rather than calling `cargo clippy` manually.
#
# TODO It would be nice to provide arguments to narrow clippy checks to a single
# workspace and/or package.  To speed up the interactive workflow.

set -o errexit

here="$(dirname "$0")"
cargo="$(readlink -f "${here}/../cargo")"

if [[ -z $cargo ]]; then
  >&2 echo "Failed to find cargo. Mac readlink doesn't support -f. Consider switching
  to gnu readlink with 'brew install coreutils' and then symlink greadlink as
  /usr/local/bin/readlink."
  exit 1
fi

# shellcheck source=ci/rust-version.sh
source "$here/../ci/rust-version.sh"

nightly_clippy_allows=(--allow=clippy::redundant_clone)

# Use nightly clippy, as frozen-abi proc-macro generates a lot of code across
# various crates in this whole monorepo (frozen-abi is enabled only under nightly
# due to the use of unstable rust feature). Likewise, frozen-abi(-macro) crates'
# unit tests are only compiled under nightly.
# Similarly, nightly is desired to run clippy over all of bench files because
# the bench itself isn't stabilized yet...
#   ref: https://github.com/rust-lang/rust/issues/66287
"$here/cargo-for-all-lock-files.sh" -- \
  "+${rust_nightly}" clippy \
  --workspace --all-targets --features dummy-for-ci-check -- \
  --deny=warnings \
  --deny=clippy::default_trait_access \
  --deny=clippy::arithmetic_side_effects \
  --deny=clippy::manual_let_else \
  --deny=clippy::used_underscore_binding \
  "${nightly_clippy_allows[@]}"

# temporarily run stable clippy as well to scan the codebase for
# `redundant_clone`s, which is disabled as nightly clippy is buggy:
#   https://github.com/solana-labs/solana/issues/31834
#
# can't use --all-targets:
#   error[E0554]: `#![feature]` may not be used on the stable release channel
"$here/cargo-for-all-lock-files.sh" -- \
  clippy \
  --workspace --tests --bins --examples --features dummy-for-ci-check -- \
  --deny=warnings \
  --deny=clippy::default_trait_access \
  --deny=clippy::arithmetic_side_effects \
  --deny=clippy::manual_let_else \
  --deny=clippy::used_underscore_binding
