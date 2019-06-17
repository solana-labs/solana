#!/usr/bin/env bash
set -ex

cd "$(dirname "$0")"

../../../bpf-sdk/rust/build.sh ../../../examples/bpf-rust-noop
cp ../../../examples/bpf-rust-noop/target/bpfel-unknown-unknown/release/solana_bpf_rust_noop.so .
