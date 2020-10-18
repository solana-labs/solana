#! /bin/bash
export LLVM_SYS_80_PREFIX=/home/g/Desktop/llvm/build
export LLVM_SYS_80_STRICT_VERSIONING=true
export RUST_LOG=debug

# FILE=/home/g/Desktop/chainlink/evm-contracts/src/v0.7/dev/Owned.sol
FILE=/home/g/Desktop/chainlink/evm-contracts/src/v0.7/dev/Operator.sol
# FILE=tests/contracts/set.sol
cargo run $FILE

# cargo run --example codegen

opt out.ll --O3 -S -o opt.ll
llc out.ll -march=bpf -o out.bpf.s -O3

llc out.ll -o out.x64.s -O3
llc out.ll -filetype=obj -o out.o -relocation-model=pic -O3
clang runtime/utils.c runtime/sha3.c runtime/rt.c  out.o -fPIC -o a.out
./a.out
