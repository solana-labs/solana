#!/usr/bin/env bash

cd "$(dirname "$0")"/..

# Install Criterion
version=v2.3.2
if [[ ! -r criterion-version.md ]]; then
  (
    if [[ "$(uname)" = Darwin ]]; then
      machine=osx
    else
      machine=linux
    fi

    set -ex
    rm -rf criterion
    mkdir criterion
    cd criterion
    wget --progress=dot:mega https://github.com/Snaipe/Criterion/releases/download/$version/criterion-$version-$machine-x86_64.tar.bz2
    tar --strip-components 1 -jxf criterion-$version-$machine-x86_64.tar.bz2
    rm -rf criterion-$version-$machine-x86_64.tar.bz2

    echo "https://github.com/Snaipe/Criterion/releases/tag/$version" > ../criterion-version.md
  )
  # shellcheck disable=SC2181
  if [[ $? -ne 0 ]]; then
    rm -rf criterion
    exit 1
  fi
fi

# Install LLVM
version=v0.0.1
if [[ ! -f llvm-native-version.md ]]; then
  (
    if [[ "$(uname)" = Darwin ]]; then
      machine=macos
    else
      machine=linux
    fi

    set -ex
    rm -rf llvm-native
    mkdir -p llvm-native
    cd llvm-native
    wget --progress=dot:giga https://github.com/solana-labs/llvm-builder/releases/download/$version/solana-llvm-$machine.tgz
    tar xzf solana-llvm-$machine.tgz
    rm -rf solana-llvm-$machine.tgz

    echo "https://github.com/solana-labs/llvm-builder/releases/tag/$version" > ../llvm-native-version.md
  )

  # shellcheck disable=SC2181
  if [[ $? -ne 0 ]]; then
    rm -rf llvm-native
    exit 1
  fi
fi

