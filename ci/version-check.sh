#!/usr/bin/env bash
set -e

require() {
  declare expectedProgram="$1"
  declare expectedVersion="$2"
  shift 2

  read -r program version _ < <($expectedProgram "$@" -V)

  declare ok=true
  [[ $program = "$expectedProgram" ]] || ok=false
  [[ $version =~ $expectedVersion ]] || ok=false

  echo "Found $program $version"
  if ! $ok; then
    echo Error: expected "$expectedProgram $expectedVersion"
    exit 1
  fi
}

case ${1:-stable} in
nightly)
  require rustc 1.34.[0-9]+-nightly +nightly
  require cargo 1.34.[0-9]+-nightly +nightly
  ;;
stable)
  require rustc 1.32.[0-9]+
  require cargo 1.32.[0-9]+
  ;;
*)
  echo Error: unknown argument: "$1"
  exit 1
  ;;
esac

exit 0
