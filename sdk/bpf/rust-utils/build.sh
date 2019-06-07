#!/usr/bin/env bash

if [ "$#" -ne 1 ]; then
    echo "Error: Must provide the full path to the project to build"
    exit 1
fi
if [ ! -f "$1/Cargo.toml" ]; then
      echo "Error: Cannot find project: $1"
    exit 1
fi

cd "$(dirname "$0")"
bpf_sdk="$PWD/.."
export XARGO_HOME="$PWD/../../../target/xargo"

cd "$1"

cargo install xargo

set -e

# Ensure the sdk is installed
"$bpf_sdk"/scripts/install.sh

# Use the SDK's version of llvm to build the compiler-builtins for BPF
export CC="$bpf_sdk/llvm-native/bin/clang"
export AR="$bpf_sdk/llvm-native/bin/llvm-ar"
# Use the SDK's version of Rust to build for BPF
export RUSTUP_TOOLCHAIN=bpf
export RUSTFLAGS="
    --emit=llvm-ir \
    -C lto=no \
    -C opt-level=2 \
    -C link-arg=-z -C link-arg=notext \
    -C link-arg=-T$bpf_sdk/rust-utils/bpf.ld \
    -C link-arg=--Bdynamic \
    -C link-arg=-shared \
    -C link-arg=--entry=entrypoint \
    -C linker=$bpf_sdk/llvm-native/bin/ld.lld"
export XARGO_RUST_SRC="$bpf_sdk/rust-bpf-sysroot/src"
xargo build --target bpfel-unknown-unknown --release -v

{ { set +x; } 2>/dev/null; echo Success; }
