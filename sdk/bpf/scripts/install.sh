#!/usr/bin/env bash

cd "$(dirname "$0")"/../dependencies

if [[ "$(uname)" = Darwin ]]; then
  machine=osx
else
  machine=linux
fi

download() {
  declare url=$1
  declare filename=$2
  declare progress=$3

  declare args=(
    "$url" -O "$filename"
    "--progress=dot:$progress"
    "--retry-connrefused"
    "--read-timeout=30"
  )
  wget "${args[@]}"
}

# Install or upgrade xargo
cargo install cargo-update 2> /dev/null
cargo install-update -i xargo
xargo --version > xargo.md 2>&1

# Install Criterion
version=v2.3.2
if [[ ! -r criterion-$machine-$version.md ]]; then
  (
    filename=criterion-$version-$machine-x86_64.tar.bz2

    set -ex
    rm -rf criterion*
    mkdir criterion
    cd criterion

    base=https://github.com/Snaipe/Criterion/releases
    download $base/download/$version/$filename $filename mega
    tar --strip-components 1 -jxf $filename
    rm -rf $filename

    echo "$base/tag/$version" > ../criterion-$machine-$version.md
  )
  # shellcheck disable=SC2181
  if [[ $? -ne 0 ]]; then
    rm -rf criterion
    exit 1
  fi
fi

# Install LLVM
version=v0.0.15
if [[ ! -f llvm-native-$machine-$version.md ]]; then
  (
    filename=solana-llvm-$machine.tar.bz2

    set -ex
    rm -rf llvm-native*
    rm -rf xargo
    mkdir -p llvm-native
    cd llvm-native

    base=https://github.com/solana-labs/llvm-builder/releases
    download $base/download/$version/$filename $filename giga
    tar -jxf $filename
    rm -rf $filename

    echo "$base/tag/$version" > ../llvm-native-$machine-$version.md
  )
  exitcode=$?
  if [[ $exitcode -ne 0 ]]; then
    rm -rf llvm-native
    exit 1
  fi
fi

# Install Rust-BPF
version=v0.2.1
if [[ ! -f rust-bpf-$machine-$version.md ]]; then
  (
    filename=solana-rust-bpf-$machine.tar.bz2

    set -ex
    rm -rf rust-bpf
    rm -rf rust-bpf-$machine-*
    rm -rf xargo
    mkdir -p rust-bpf
    pushd rust-bpf

    base=https://github.com/solana-labs/rust-bpf-builder/releases
    download $base/download/$version/$filename $filename giga
    tar -jxf $filename
    rm -rf $filename
    popd

    set -ex
    ./rust-bpf/bin/rustc --print sysroot

    set +e
    rustup toolchain uninstall bpf
    set -e
    rustup toolchain link bpf rust-bpf

    echo "$base/tag/$version" > rust-bpf-$machine-$version.md
  )
  exitcode=$?
  if [[ $exitcode -ne 0 ]]; then
    rm -rf rust-bpf
    exit 1
  fi
fi

# Install Rust-BPF Sysroot sources
version=v0.12
if [[ ! -f rust-bpf-sysroot-$version.md ]]; then
  (
    set -ex
    rm -rf rust-bpf-sysroot*
    rm -rf xargo
    cmd="git clone --recursive --single-branch --branch $version https://github.com/solana-labs/rust-bpf-sysroot.git"
    $cmd

    echo "$cmd" > rust-bpf-sysroot-$version.md
  )
  exitcode=$?
  if [[ $exitcode -ne 0 ]]; then
    rm -rf rust-bpf-sysroot
    exit 1
  fi
fi

exit 0
