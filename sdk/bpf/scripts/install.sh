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
version=c3580fad71d76c451e22db84395c127c8a773afd
if [[ ! -f rust-bpf-sysroot-$version.md ]]; then
  (
    filename=solana-rust-bpf-sysroot.tar.bz2

    set -ex
    rm -rf rust-bpf-sysroot*
    mkdir -p rust-bpf-sysroot
    cd rust-bpf-sysroot

    git init
    git remote add origin https://github.com/solana-labs/rust-bpf-sysroot.git
    git pull origin master
    git checkout "$version"
    git submodule init
    git submodule update

    echo "https://github.com/solana-labs/rust-bpf-sysroot/releases/tag/$version" > ../rust-bpf-sysroot-$version.md
  )
  exitcode=$?
  if [[ $exitcode -ne 0 ]]; then
    rm -rf rust-bpf-sysroot
    exit 1
  fi
fi

exit 0
