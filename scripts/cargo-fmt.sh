#!/usr/bin/env bash

here="$(dirname "$0")"
cargo="$(readlink -f "${here}/../cargo")"

if [[ -z $cargo ]]; then
  >&2 echo "Failed to find cargo. Mac readlink doesn't support -f. Consider switching
  to gnu readlink with 'brew install coreutils' and then symlink greadlink as
  /usr/local/bin/readlink."
  exit 1
fi

fmt_dirs=(
  .
  programs/sbf
  sdk/cargo-build-sbf/tests/crates/fail
  sdk/cargo-build-sbf/tests/crates/noop
  storage-bigtable/build-proto
)

for fmt_dir in "${fmt_dirs[@]}"; do
  (
    manifest_path="$(readlink -f "$here"/../"$fmt_dir"/Cargo.toml)"
    set -ex
    "$cargo" nightly fmt --all --manifest-path "$manifest_path"
  )
done
