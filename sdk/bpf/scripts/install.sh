#!/usr/bin/env bash

cd "$(dirname "$0")"/..

if [[ "$(uname)" = Darwin ]]; then
  machine=osx
else
  machine=linux
fi

# Install Criterion
version=v2.3.2
if [[ ! -r criterion-$machine-$version.md ]]; then
  (
    filename=criterion-$version-$machine-x86_64.tar.bz2

    set -ex
    rm -rf criterion*
    mkdir criterion
    cd criterion
    wget --progress=dot:mega https://github.com/Snaipe/Criterion/releases/download/$version/$filename
    tar --strip-components 1 -jxf $filename
    rm -rf $filename

    echo "https://github.com/Snaipe/Criterion/releases/tag/$version" > ../criterion-$machine-$version.md
  )
  # shellcheck disable=SC2181
  if [[ $? -ne 0 ]]; then
    rm -rf criterion
    exit 1
  fi
fi

# Install LLVM
version=v0.0.6
if [[ ! -f llvm-native-$machine-$version.md ]]; then
  (
    filename=solana-llvm-$machine.tar.bz2

    set -ex
    rm -rf llvm-native*
    mkdir -p llvm-native
    cd llvm-native
    wget --progress=dot:giga https://github.com/solana-labs/llvm-builder/releases/download/$version/$filename
    tar -jxf $filename
    rm -rf $filename

    echo "https://github.com/solana-labs/llvm-builder/releases/tag/$version" > ../llvm-native-$machine-$version.md
  )
  # shellcheck disable=SC2181
  if [[ $? -ne 0 ]]; then
    rm -rf llvm-native
    exit 1
  fi
fi

