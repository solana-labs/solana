#!/usr/bin/env bash

here="$(dirname "$0")"
cargoNightly="$(readlink -f "${here}/../cargo-nightly")"

if [[ -z $cargoNightly ]]; then
  >&2 echo "Failed to find cargo. Mac readlink doesn't support -f. Consider switching
  to gnu readlink with 'brew install coreutils' and then symlink greadlink as
  /usr/local/bin/readlink."
  exit 1
fi

set -ex

"$cargoNightly" fmt --all
(cd programs/sbf && "$cargoNightly" fmt --all)
(cd sdk/cargo-build-sbf/tests/crates/fail && "$cargoNightly" fmt --all)
(cd sdk/cargo-build-sbf/tests/crates/noop && "$cargoNightly" fmt --all)
(cd storage-bigtable/build-proto && "$cargoNightly" fmt --all)
(cd web3.js/test/fixtures/noop-program && "$cargoNightly" fmt --all)
