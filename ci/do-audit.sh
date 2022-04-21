#!/usr/bin/env bash

set -e

here="$(dirname "$0")"
src_root="$(readlink -f "${here}/..")"

cd "${src_root}"

cargo_audit_ignores=(
  # `net2` crate has been deprecated; use `socket2` instead
  #
  # Blocked on https://github.com/paritytech/jsonrpc/issues/575
  --ignore RUSTSEC-2020-0016

  # stdweb is unmaintained
  #
  # Blocked on multiple upstream crates removing their `stdweb` dependency.
  --ignore RUSTSEC-2020-0056

  # Potential segfault in the time crate
  #
  # Blocked on multiple crates updating `time` to >= 0.2.23
  --ignore RUSTSEC-2020-0071

  # chrono: Potential segfault in `localtime_r` invocations
  #
  # Blocked due to no safe upgrade
  # https://github.com/chronotope/chrono/issues/499
  --ignore RUSTSEC-2020-0159

)
scripts/cargo-for-all-lock-files.sh stable audit "${cargo_audit_ignores[@]}"
