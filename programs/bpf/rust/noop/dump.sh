#!/usr/bin/env bash

cp dump.txt dump_last.txt 2>/dev/null

set -x
set -e

./clean.sh
./build.sh
ls -la ./target/bpfel-unknown-unknown/release/solana_bpf_rust_noop.so > dump.txt
greadelf -aW ./target/bpfel-unknown-unknown/release/solana_bpf_rust_noop.so | rustfilt >> dump.txt
llvm-objdump -print-imm-hex --source --disassemble ./target/bpfel-unknown-unknown/release/solana_bpf_rust_noop.so >> dump.txt
