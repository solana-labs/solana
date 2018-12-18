#!/usr/bin/env bash
#
# Audits project dependencies for security vulnerabilities
#
set -e

cd "$(dirname "$0")/.."
source ci/_

cargo_install_unless() {
  declare crate=$1
  shift

  "$@" > /dev/null 2>&1 || \
    _ cargo install "$crate"
}

cargo_install_unless cargo-audit cargo audit --version

_ cargo audit
