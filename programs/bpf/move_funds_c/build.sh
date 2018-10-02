#!/bin/sh

set -e
set -x

mkdir -p out
/usr/local/opt/llvm/bin/clang -target bpf -O2 -emit-llvm -fno-builtin -o ../../../target/release/move_funds_c.bc -c src/move_funds.c
/usr/local/opt/llvm/bin/llc -march=bpf -filetype=obj -function-sections -o ../../../target/release/move_funds_c.o ../../../target/release/move_funds_c.bc

/usr/local/opt/llvm/bin/llvm-objdump -color -source -disassemble ../../../target/release/move_funds_c.o