#!/usr/bin/env bash

here="$(dirname "$0")"
cargo="$(readlink -f "${here}/../cargo")"

if [[ -z $cargo ]]; then
  >&2 echo "Failed to find cargo. Mac readlink doesn't support -f. Consider switching
  to gnu readlink with 'brew install coreutils' and then symlink greadlink as
  /usr/local/bin/readlink."
  exit 1
fi

set -ex

"$cargo" nightly fmt --all
(cd programs/bpf && "$cargo" nightly fmt --all)
(cd sdk/cargo-build-sbf/tests/crates/fail && "$cargo" nightly fmt --all)
(cd sdk/cargo-build-sbf/tests/crates/noop && "$cargo" nightly fmt --all)
(cd storage-bigtable/build-proto && "$cargo" nightly fmt --all)
(cd web3.js/test/fixtures/noop-program && "$cargo" nightly fmt --all)
