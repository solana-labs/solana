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

set -x
mkdir -p "$installDir/bin/deps"
cp -fv "$programDir"/target/release/deps/libsolana*program.* "$installDir/bin/deps"
