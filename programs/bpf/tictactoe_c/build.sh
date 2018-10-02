#!/bin/sh

set -e
set -x

mkdir -p ../../../target/release/
/usr/local/opt/llvm/bin/clang -target bpf -O2 -emit-llvm -fno-builtin -o ../../../target/release/tictactoe_c.bc -c src/tictactoe.c
/usr/local/opt/llvm/bin/llc -march=bpf -filetype=obj -function-sections -o ../../../target/release/tictactoe_c.o ../../../target/release/tictactoe_c.bc

/usr/local/opt/llvm/bin/llvm-objdump -color -source -disassemble ../../../target/release/tictactoe_c.o