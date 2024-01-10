#!/usr/bin/env bash

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
source "$here/../ci/rust-version.sh" stable

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
