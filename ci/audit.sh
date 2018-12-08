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
  set -x
  declare crate=$1
  shift

  exit=0 && "$@" > /dev/null 2>&1 || exit=$?

  # remove when https://github.com/RustSec/cargo-audit/issues/56
  #  is resolved
  [[ $crate == cargo-audit ]] && (( exit == 2 )) && exit=0

  if (( exit )) ; then
    _ cargo install "$crate"
  fi

}

maybe_cargo_install cargo-audit cargo audit --help
maybe_cargo_install cargo-tree cargo tree --help

_ cargo tree
_ cargo audit
