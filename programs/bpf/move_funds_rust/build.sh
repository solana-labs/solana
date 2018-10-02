#!/bin/sh

set -e
set -x

#-C llvm-args='-fno-builtin'
mkdir -p ../../../target/release/
rm -f ../../../target/release/deps/*.ll
rm -f ../../../target/release/deps/*.bc
cargo +nightly rustc --release -- -C panic=abort -C opt-level=0 --emit=llvm-ir
cargo +nightly  rustc --release -- -C panic=abort -C opt-level=0 --emit=llvm-bc
cp ../../../target/release/deps/move_funds_rust-*.bc ../../../target/release/move_funds_rust.bc
#/usr/local/opt/llvm/bin/llc -march=bpf -filetype=obj -o ../../../target/release/move_funds_rust.o ../../../target/release/move_funds_rust.bc
/Users/jack/Downloads/clang+llvm-7.0.0-x86_64-apple-darwin/bin/llc -march=bpf -filetype=obj -o ../../../target/release/move_funds_rust.o ../../../target/release/move_funds_rust.bc

#rustc --emit=llvm-ir -C opt-level=3 -o ../../../target/release/move_funds_rust.bc src/lib.rs
#/usr/local/opt/llvm/bin/llc -march=bpf -filetype=obj -function-sections -o ../../../target/release/move_funds_rust.o ../../../target/release/move_funds_rust.bc