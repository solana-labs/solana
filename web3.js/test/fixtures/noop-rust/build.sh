#!/usr/bin/env bash
set -ex

cd "$(dirname "$0")"

cargo build-bpf --manifest-path=../../../examples/bpf-rust-noop/Cargo.toml
cp ../../../examples/bpf-rust-noop/target/deploy/solana_bpf_rust_noop.so .
