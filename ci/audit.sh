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
  declare cmd=$1
  declare crate=${2:-$cmd}

  "$cmd" --help > /dev/null 2>&1 || \
    _ cargo install "$crate"
}

maybe_cargo_install audit tree

_ cargo tree
_ cargo audit
