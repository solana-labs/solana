#!/usr/bin/env bash
#
# Run tests and collect code coverage
#
# == Usage:
#
# Run all:
#   $ ./script/coverage.sh
#
# Run for specific packages
#   $ ./script/coverage.sh -p solana-account-decoder
#   $ ./script/coverage.sh -p solana-account-decoder -p solana-accounts-db [-p ...]
#
# Custom folder name. (default: $(git rev-parse --short=9 HEAD))
#   $ COMMIT_HASH=xxx ./script/coverage.sh -p solana-account-decoder
#

set -e
here=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" &>/dev/null && pwd)

# Check for grcov commands
if ! command -v grcov >/dev/null 2>&1; then
  echo "Error: grcov not found.  Try |cargo install grcov|"
  exit 1
fi

# Use nightly as we have some nightly-only tests (frozen-abi)
# shellcheck source=ci/rust-version.sh
source "$here/../ci/rust-version.sh" nightly

# get commit hash. it will be used to name output folder
if [ -z "$COMMIT_HASH" ]; then
  COMMIT_HASH=$(git rev-parse --short=9 HEAD)
fi

# Clean up
rm -rf "$here/../target/cov/$COMMIT_HASH"
find "$here/.." -type f -name '*.prof*' -exec rm {} +
find "$here/.." -type f -name '*lcov*' -exec rm {} +

# https://doc.rust-lang.org/rustc/instrument-coverage.html
export RUSTFLAGS="-C instrument-coverage $RUSTFLAGS"
export LLVM_PROFILE_FILE="$here/../target/cov/${COMMIT_HASH}/profraw/default-%p-%m.profraw"

if [[ -z $1 ]]; then
  PACKAGES=(--lib --all --exclude solana-local-cluster)
else
  PACKAGES=("$@")
fi

TEST_ARGS=(
  --skip shred::merkle::test::test_make_shreds_from_data::
  --skip shred::merkle::test::test_make_shreds_from_data_rand::
  --skip shred::merkle::test::test_recover_merkle_shreds::
)

cargo +"$rust_nightly" test --target-dir "$here/../target/cov" "${PACKAGES[@]}" -- "${TEST_ARGS[@]}"

# Generate test reports
echo "--- grcov"
grcov_common_args=(
  "$here/.."
  --source-dir "$here/.."
  --binary-path "$here/../target/cov/debug"
  --llvm
  --ignore \*.cargo\*
  --ignore \*build.rs
  --ignore bench-tps\*
  --ignore upload-perf\*
  --ignore bench-streamer\*
  --ignore local-cluster\*
  --ignore-not-existing
)

grcov "${grcov_common_args[@]}" -t html -o "$here/../target/cov/${COMMIT_HASH}/coverage/html"
echo "html: $here/../target/cov/${COMMIT_HASH}/coverage/html"

grcov "${grcov_common_args[@]}" -t lcov -o "$here/../target/cov/${COMMIT_HASH}/coverage/lcov.info"
echo "lcov: $here/../target/cov/${COMMIT_HASH}/coverage/lcov.info"

ln -sfT "$here/../target/cov/${COMMIT_HASH}" "$here/../target/cov/LATEST"
