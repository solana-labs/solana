#!/usr/bin/env bash

set -ex

export RUSTFLAGS="$RUSTFLAGS \
    -C lto=no -C opt-level=2 \
    -C link-arg=-Tbpf.ld \
    -C link-arg=-z -C link-arg=notext \
    -C link-arg=--Bdynamic \
    -C link-arg=-shared \
    -C link-arg=--entry=entrypoint \
     --sysroot ../../../../sdk/bpf/rust-bpf-sysroot"

export CARGO_TARGET_BPFEL_UNKNOWN_UNKNOWN_LINKER="../../../../sdk/bpf/llvm-native/bin/ld.lld"

# Ensure the sdk is installed
../../../../sdk/bpf/scripts/install.sh

cargo +bpf build --release --target=bpfel_unknown_unknown -v

{ { set +x; } 2>/dev/null; echo Success; }