#!/bin/bash -ex

OUTDIR="${1:-../../../target/release/}"
THISDIR=$(dirname "$0")
mkdir -p "$OUTDIR"
/usr/local/opt/llvm/bin/clang -Werror -target bpf -O2 -emit-llvm -fno-builtin -o "$OUTDIR"/move_funds_c.bc -c "$THISDIR"/src/move_funds.c
/usr/local/opt/llvm/bin/llc -march=bpf -filetype=obj -function-sections -o "$OUTDIR"/move_funds_c.o "$OUTDIR"/move_funds_c.bc

#/usr/local/opt/llvm/bin/llvm-objdump -color -source -disassemble "$OUTDIR"/move_funds_c.o