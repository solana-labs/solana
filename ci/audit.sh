#!/usr/bin/env bash
#
# Audits project dependencies for security vulnerabilities
#
set -e

cd "$(dirname "$0")/.."

export RUST_BACKTRACE=1
rustc --version
cargo --version

_() {
  echo "--- $*"
  "$@"
}

maybe_cargo_install() {
  for cmd in "$@"; do
    set +e
    cargo "$cmd" --help > /dev/null 2>&1
    declare exitcode=$?
    set -e
    if [[ $exitcode -eq 101 ]]; then
      _ cargo install cargo-"$cmd"
    fi
  done
}

maybe_cargo_install audit tree

_ cargo tree
_ cargo audit
