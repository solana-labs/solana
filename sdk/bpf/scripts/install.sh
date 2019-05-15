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
version=v0.0.9
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
  exitcode=$?
  if [[ $exitcode -ne 0 ]]; then
    rm -rf llvm-native
    exit 1
  fi
fi

# Install Rust-BPF
version=v0.0.2
if [[ ! -f rust-bpf-$machine-$version.md ]]; then
  (
    filename=solana-rust-bpf-$machine.tar.bz2

    set -ex
    rm -rf rust-bpf
    rm -rf rust-bpf-$machine-*
    mkdir -p rust-bpf
    pushd rust-bpf
    wget --progress=dot:giga https://github.com/solana-labs/rust-bpf-builder/releases/download/$version/$filename
    tar -jxf $filename
    rm -rf $filename
    popd

    set -ex
    ./rust-bpf/bin/rustc --print sysroot

    set +e
    rustup toolchain uninstall bpf
    set -e
    rustup toolchain link bpf rust-bpf

    echo "https://github.com/solana-labs/rust-bpf-builder/releases/tag/$version" > rust-bpf-$machine-$version.md
  )
  exitcode=$?
  if [[ $exitcode -ne 0 ]]; then
    rm -rf rust-bpf
    exit 1
  fi
fi

# Install Rust-BPF Sysroot sources
version=v0.1
if [[ ! -f rust-bpf-sysroot-$version.md ]]; then
  (
    filename=solana-rust-bpf-sysroot.tar.bz2

    set -ex
    rm -rf rust-bpf-sysroot*
    git clone --recursive --single-branch --branch $version git@github.com:solana-labs/rust-bpf-sysroot.git

    echo "git clone --recursive --single-branch --branch $version git@github.com:solana-labs/rust-bpf-sysroot.git" > rust-bpf-sysroot-$version.md
  )
  exitcode=$?
  if [[ $exitcode -ne 0 ]]; then
    rm -rf rust-bpf-sysroot
    exit 1
  fi
fi

exit 0
