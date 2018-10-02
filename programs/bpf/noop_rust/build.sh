#!/bin/sh

set -e
set -x

# cargo +nightly rustc --release -- -C panic=abort --emit=llvm-ir
cargo +nightly  rustc --release -- -C panic=abort --emit=llvm-bc
cp ../../../target/release/deps/noop_rust-*.bc ../../../target/release/noop_rust.bc
/usr/local/opt/llvm/bin/llc -march=bpf -filetype=obj -o ../../../target/release/noop_rust.o ../../../target/release/noop_rust.bc