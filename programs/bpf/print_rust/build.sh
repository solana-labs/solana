#!/bin/sh

set -e
set -x

mkdir -p ../../../target/release/
# cargo +nightly rustc --release -- -C panic=abort --emit=llvm-ir
cargo +nightly  rustc --release -- -C panic=abort --emit=llvm-bc
cp ../../../target/release/deps/print_rust-*.bc ../../../target/release/print_rust.bc
/usr/local/opt/llvm/bin/llc -march=bpf -filetype=obj -o ../../../target/release/print_rust.o ../../../target/release/print_rust.bc