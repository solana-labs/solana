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

cargo_install_unless() {
  declare crate=$1
  shift

  "$@" > /dev/null 2>&1 || \
    _ cargo install "$crate"
}

cargo_install_unless cargo-audit cargo audit --version

_ cargo audit
