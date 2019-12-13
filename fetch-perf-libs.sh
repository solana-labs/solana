#!/usr/bin/env bash

PERF_LIBS_VERSION=v0.16.2
VERSION=$PERF_LIBS_VERSION-1

set -e
cd "$(dirname "$0")"

if [[ ! -f target/perf-libs/.$VERSION ]]; then
  if [[ $(uname) != Linux ]]; then
    echo Note: Performance libraries are only available for Linux
    exit 0
  fi

  if [[ $(uname -m) != x86_64 ]]; then
    echo Note: Performance libraries are only available for x86_64 architecture
    exit 0
  fi

  mkdir -p target/perf-libs
  (
    set -x
    cd target/perf-libs
    curl -L --retry 5 --retry-delay 2 --retry-connrefused -o solana-perf.tgz \
      https://github.com/solana-labs/solana-perf-libs/releases/download/$PERF_LIBS_VERSION/solana-perf.tgz
    tar zxvf solana-perf.tgz
    rm -f solana-perf.tgz
    touch .$VERSION
  )

  # Setup symlinks so the perf-libs/ can be found from all binaries run out of
  # target/
  for dir in target/{debug,release}/{,deps/}; do
    mkdir -p $dir
    ln -sfT ../perf-libs ${dir}perf-libs
  done

fi

exit 0
