#!/usr/bin/env bash
#
# This script will install all cargo workspace libraries found in
# `programDir` as native programs.
set -e

# Directory to install libraries into
installDir="$(mkdir -p "$1"; cd "$1"; pwd)"

# Where to find custom programs
programDir="$2"

(
  set -x
  cd "$programDir"
  cargo build --release
)

for dir in "$programDir"/*; do
  for program in $programDir/target/release/deps/lib"$(basename "$dir")".{so,dylib,dll}; do
    if [[ -f $program ]]; then
      (
        set -x
        mkdir -p "$installDir/bin/deps"
        rm -f "$installDir/bin/deps/$(basename "$program")"
        cp -v "$program" "$installDir"/bin/deps
      )
    fi
  done
done
