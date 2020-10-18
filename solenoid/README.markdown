# Solenoid compiler

Solenoid compiler uses LLVM to translate EVM to Solana's BPF VM. It uses solc to compile contracts into EVM opcodes, gets the constructor and runtime payload, their associated ABI, compile into LLVM IR.

The output is a module containing the following functions:

1. contract constructor
2. contract runtime
2. abi conversion functions

It also consists of a runtime. To compile the .ll file into BPF you will need `solana-labs/llvm`.

## How to run

```
cargo install
./test.sh
```