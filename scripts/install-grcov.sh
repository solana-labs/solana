#!/usr/bin/env bash
#
# runs grcov, installs if not already here
#

cd "$(dirname "$0")/.."
source ci/_

tmpdir=/tmp/install-grcov-$$

(mkdir -p "$tmpdir" &&
      touch "$tmpdir"/empty.gcno &&
      grcov "$tmpdir" >/dev/null 2>&1) || _ cargo install grcov

rm -rf "$tmpdir"
