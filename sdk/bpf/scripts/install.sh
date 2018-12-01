#!/usr/bin/env bash

cd "$(dirname "$0")"/..

# Install Criterion for all supported platforms
# if changing version here must also change in bpf.mk
version=v2.3.2
if [[ ! -d criterion-$version ]]; then
  (
    if [[ "$(uname)" = Darwin ]]; then
      machine=osx
    else
      machine=linux
    fi

    set -ex
    wget --progress=dot:mega https://github.com/Snaipe/Criterion/releases/download/$version/criterion-$version-$machine-x86_64.tar.bz2
    tar jxf criterion-$version-$machine-x86_64.tar.bz2
    rm -rf criterion-$version-$machine-x86_64.tar.bz2

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
# if changing version here must also change in bpf.mk
version=v0.0.1
if [[ ! -d llvm/native-$version ]]; then
  (
    if [[ "$(uname)" = Darwin ]]; then
      machine=macos
    else
      machine=linux
    fi

    set -ex
    mkdir -p llvm/native-$version
    cd llvm/native-$version
    wget --progress=dot:giga https://github.com/solana-labs/llvm-builder/releases/download/$version/solana-llvm-$machine.tgz
    tar xzf solana-llvm-$machine.tgz
    rm -rf solana-llvm-$machine.tgz

    [[ ! -f llvm/native-$version/README.md ]]
    echo "https://github.com/solana-labs/llvm-builder/releases/tag/$version" > README.md
  )

  # shellcheck disable=SC2181
  if [[ $? -ne 0 ]]; then
    rm -rf llvm/native-$version
    exit 1
  fi
fi

