#! /bin/bash

LLVM_SYS_80_PREFIX=/mnt/c/Users/ricky/Desktop/llvm/build LLVM_SYS_80_STRICT_VERSIONING=true cargo build
RUST_LOG=warn ./target/debug/solenoid tests/contracts/set.sol
# LLVM_SYS_80_PREFIX=/home/g/Desktop/llvm/build LLVM_SYS_80_STRICT_VERSIONING=true cargo test

../llvm/build/bin/opt out.ll --O3 -S -o opt.ll
../llvm/build/bin/llc opt.ll -march=bpf -o out.bpf.s -O3

llc out.ll -o out.x64.s -O3
llc out.ll -filetype=obj -o out.o -relocation-model=pic -O3
clang runtime/utils.c runtime/sha3.c runtime/rt.c  out.o -fPIC -o a.out
./a.out
