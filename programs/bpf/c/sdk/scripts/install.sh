#!/usr/bin/env bash

cd "$(dirname "$0")"/..

# Install Criterion for all supported platforms
version=v2.3.2
if [[ ! -d criterion-$version ]]; then
  (
    [[ ! -d criterion-$version ]] || exit 0
    set -ex
    wget https://github.com/Snaipe/Criterion/releases/download/$version/criterion-$version-osx-x86_64.tar.bz2
    wget https://github.com/Snaipe/Criterion/releases/download/$version/criterion-$version-linux-x86_64.tar.bz2
    tar jxf criterion-$version-osx-x86_64.tar.bz2
    tar jxf criterion-$version-linux-x86_64.tar.bz2
    rm -rf criterion-$version-osx-x86_64.tar.bz2 criterion-$version-linux-x86_64.tar.bz2

    [[ ! -f criterion-$version/README.md ]]
    echo "https://github.com/Snaipe/Criterion/releases/tag/$version" > criterion-$version/README.md
  )
  # shellcheck disable=SC2181
  if [[ $? -ne 0 ]]; then
    rm -rf criterion-$version*
    exit 1
  fi
fi

# Install LLVM
version=v0.0.1
if [ "$(uname)" == "Darwin" ]; then
  machine=macos
else
  machine=linux
fi
if [[ ! -d llvm/native-$version ]]; then
  (
    set -ex
    mkdir -p llvm/native-$version
    cd llvm/native-$version
    wget https://github.com/solana-labs/llvm-builder/releases/download/$version/solana-llvm-$machine.tgz
    tar xvzf solana-llvm-$machine.tgz
    rm -rf solana-llvm-$machine.tgz

    [[ ! -f llvm/native-$version/README.md ]]
    echo "https://github.com/solana-labs/solana-web3.js/releases/tag/$version" > README.md
  )
fi
# shellcheck disable=SC2181
if [[ $? -ne 0 ]]; then
  rm -rf llvm/native-$version
  exit 1
fi

