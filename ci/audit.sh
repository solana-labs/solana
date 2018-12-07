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
  for i in "$@"; do
      declare cmd=${i%:*}
      declare crate=${i#*:}

      exit=0 && "$cmd" --help > /dev/null 2>&1 || exit=$?

      # remove when https://github.com/RustSec/cargo-audit/issues/56
      #  is resolved
      [[ $cmd == cargo-audit ]] && (( exit == 2 )) && exit=0

      if (( exit )) ; then
          _ cargo install "$crate"
      fi
  done
}

maybe_cargo_install cargo-audit cargo-tree

_ cargo tree
_ cargo audit
