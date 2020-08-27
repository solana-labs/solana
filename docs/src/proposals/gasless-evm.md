---
title: Gasless EVM
---

## Problem

Applications targeting other blockchains use audited smart contracts
implemented in Solidity. Running that same application targeting Solana
requires migrating the smart contract to Rust and the Sealevel parallel
programming model. Creating the same smart contract in Solana requires
mechanical transformations of high-level Solidity constructs to lower-level
Rust equivalents. The process is tedious and allows for human error, requiring
a second audit of the new code. Automating the process would save time and
alleviate the need for an audit.

## Proposed Solution

Add an EVM bytecode loader. Map Solidity contract storage to Solana account
data. Map runtime data to Solana sysvars. Require transactions to specify which
accounts they may access, such that Sealevel may execute independent contracts
in parallel. Instead of requiring "gas", map EVM bytecode to BPF so that
contracts are subject to the BPF instruction limit. Support Solidity method
calls from arbitrary Solana programs and executing Solana instructions from
Solidity methods.

### Developer Experience

```bash
solc --bin sourceFile.sol > lib-hello.bin
solana-keygen new -o lib-hello.json
solana deploy lib-hello.json lib-hello.bin
```

```bash
solana-keygen new -o hello-world.json
solana-evm lib-hello.json HelloWorld "Hello world!" -a hello-world.json
solana-evm lib-hello.json update "Goodbye cruel world!" -a hello-world.json
```

### EvmInstruction

```
///   0. `[]` Clock sysvar
///   1. `[]` Contract library containing method
///   *. `[writable]` Contract accounts
Invoke { method_id: Hash, args: Vec<u8> },
```

### Contract to account mapping

A Solana instruction includes a list of public keys corresponding to any account
it references. The EVM then receives an ordered key-value store, mapping those
public keys to accounts, which we'll call *account store*.

Let's use the `HelloWorld` contract to show how the account store is laid out:

```solidity
pragma solidity ^0.7.0;

contract HelloWorld {
    string public message;

    constructor(string memory initMessage) public {
        message = initMessage;
    }

    function update(string memory newMessage) public {
        message = newMessage;
    }
}
```

To call the constructor, the contract is sent an
`EvmInstruction::Invoke` that contains a hash of the contract name, and a
serialized string in `EvmInstruction::args`.

When the EVM starts executing the `HelloWorld` constructor, it uses the first
account from the account store to host all member variables. Since the user
controls what accounts are passed in, the account must also hold type
information, including the address of the contract library, and a hash of the
contract name.

When the `update()` function is called, the EVM reallocates memory for the
`HelloWorld` account, as-needed, and copies `newMessage` into it.

### Runtime data as sysvars mapping

The EVM has special instructions to access runtime data, such as TIMESTAMP. To
access the timestamp, the EVM will query the account store for
`sysvar::clock::id()`.

### Going gasless

Ethereum uses *gas* to ensure a smart contract terminates. In Solana, a
contract is allowed to run up to `N` BPF instructions, where `N` is set by the
protocol. Therefore, when the Solidity contract sends gas to other contracts,
the Solana EVM simply ignores it.

### Calling Solidity methods from Solana programs

Since Solidity methods are invoked with Solana instructions, those instructions
may be invoked from any Solana program using its cross-program invocation
mechanism.

### Executing Solana instructions from Solidity methods

The EVM is extended with a built-in function `invoke` that calls the
cross-program invocation mechanism.

### Long-running smart contracts

Long-running smart contracts are not supported by the gasless EVM. Instead, the
smart contract would need to be executed on a traditional EVM, which is outside
the scope of this proposal.

### Technical risks

* 256-bit EVM instructions may hit the BPF instruction limit so quickly that
  only the smallest of Solidity smart contracts would work
* Solana's UDP packets must stay under the MTU limit, 1,500 bytes. Complex
  Solidity contracts may implicitly reference many accounts and therefore
  exceed that limit.
