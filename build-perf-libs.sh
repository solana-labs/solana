#!/usr/bin/env bash
#
# Builds perf-libs from the upstream source and installs them into the correct
# location in the tree
#
set -e
cd "$(dirname "$0")"

if [[ -d target/perf-libs ]]; then
  echo "target/perf-libs/ already exists, to continue run:"
  echo "$ rm -rf target/perf-libs"
  exit 1
fi

(
  set -x
  git clone git@github.com:solana-labs/solana-perf-libs.git target/perf-libs
  cd target/perf-libs
  make -j"$(nproc)"
  make DESTDIR=. install
)

./fetch-perf-libs.sh
