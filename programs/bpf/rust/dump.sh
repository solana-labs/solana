#!/usr/bin/env bash

if [ "$#" -ne 1 ]; then
    echo "Error: Must provide the full path to the project to dump"
    exit 1
fi

./clean.sh "$1"
./build.sh "$1"

cd "$1"

cp ./target/dump.txt ./targetdump-last.txt 2>/dev/null

set -ex

ls -la ./target/bpfel-unknown-unknown/release/solana_bpf_rust_"$1".so > ./target/dump_mangled.txt
greadelf -aW ./target/bpfel-unknown-unknown/release/solana_bpf_rust_"$1".so >> ./target/dump_mangled.txt
llvm-objdump -print-imm-hex --source --disassemble ./target/bpfel-unknown-unknown/release/solana_bpf_rust_"$1".so >> ./target/dump_mangled.txt
sed s/://g < ./target/dump_mangled.txt | rustfilt > ./target/dump.txt

