#!/bin/sh

/usr/local/opt/llvm/bin/llvm-objdump -color -source -disassemble target/release/noop_rust.o