#!/usr/bin/env bash
set -e

cd "$(dirname "$0")"

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

export PATH=$CARGO_HOME/bin:$PATH
maybe_cargo_install mdbook
maybe_cargo_install svgbob svgbob_cli

_ make
