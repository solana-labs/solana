#!/usr/bin/env bash
set -e

cd "$(dirname "$0")"

_() {
  echo "--- $*"
  "$@"
}

maybe_cargo_install() {
  for i in "$@"; do
      declare cmd=${i%:*}
      declare crate=${i#*:}

      "$cmd" --help > /dev/null 2>&1 || \
         _ cargo install "$crate"
  done
}

export PATH=$CARGO_HOME/bin:$PATH
maybe_cargo_install mdbook svgbob:svgbob_cli

_ make
