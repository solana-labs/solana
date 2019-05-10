#!/usr/bin/env bash
set -ex

cd "$(dirname "$0")"

../../../examples/bpf-rust-noop/build.sh
cp ../../../examples/bpf-rust-noop/target/bpfel-unknown-unknown/release/solana_bpf_rust_noop.so .
