#!/bin/bash -ex

/usr/local/opt/llvm/bin/clang -Werror -target bpf -O2 -emit-llvm -fno-builtin -o noop_c.bc -c noop.c
/usr/local/opt/llvm/bin/llc -march=bpf -filetype=obj -function-sections -o noop_c.o noop_c.bc
