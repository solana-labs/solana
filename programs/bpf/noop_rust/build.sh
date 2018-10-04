#!/bin/bash -ex

# TODO building release flavor with rust produces a bunch of output .bc files
INTERDIR=../../../target/release
OUTDIR="${1:-../../../target/debug/}"
mkdir -p "$OUTDIR"
# cargo +nightly rustc --release -- -C panic=abort --emit=llvm-ir
cargo +nightly rustc --release -- -C panic=abort --emit=llvm-bc
cp "$INTERDIR"/deps/noop_rust-*.bc "$OUTDIR"/noop_rust.bc 
/usr/local/opt/llvm/bin/llc -march=bpf -filetype=obj -o "$OUTDIR"/noop_rust.o "$OUTDIR"/noop_rust.bc