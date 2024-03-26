#!/usr/bin/env bash

set -o errexit

here="$(dirname "$0")"
cargo="$(readlink -f "${here}/../cargo")"

if [[ -z $cargo ]]; then
  echo >&2 "Failed to find cargo. Mac readlink doesn't support -f. Consider switching
  to gnu readlink with 'brew install coreutils' and then symlink greadlink as
  /usr/local/bin/readlink."
  exit 1
fi

# shellcheck source=ci/rust-version.sh
source "$here/../ci/rust-version.sh" nightly

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
  --allow=clippy::redundant_clone
