#!/bin/bash -e

cd "$(dirname "$0")/.."

ci/version-check.sh stable
export RUST_BACKTRACE=1

_() {
  echo "--- $*"
  "$@"
}

_ cargo bench --verbose
