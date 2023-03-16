#!/usr/bin/env bash

set -e

here="$(dirname "$0")"
src_root="$(readlink -f "${here}/..")"

cd "${src_root}"

# `cargo-audit` doesn't give us a way to do this nicely, so hammer it is...
dep_tree_filter="grep -Ev '│|└|├|─'"

while [[ -n $1 ]]; do
  if [[ $1 = "--display-dependency-trees" ]]; then
    dep_tree_filter="cat"
    shift
  fi
done

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

  # tokio: vulnerability affecting named pipes on Windows
  #
  # Not worth upgrading tokio version on a stable branch
  --ignore RUSTSEC-2023-0001
)
<<<<<<< HEAD
scripts/cargo-for-all-lock-files.sh stable audit "${cargo_audit_ignores[@]}" | $dep_tree_filter
=======
scripts/cargo-for-all-lock-files.sh audit "${cargo_audit_ignores[@]}" | $dep_tree_filter
# we want the `cargo audit` exit code, not `$dep_tree_filter`'s
exit "${PIPESTATUS[0]}"
>>>>>>> f8abc8df8 (ci: fix do-audit don't report error (#30728))
