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
solc --bin sourceFile.sol > hello-world.bin
solana-keygen new -o hello-world.json
solana deploy hello-world.json hello-world.bin
```

```bash
solana-keygen new -o greeting.json
solana evm hello-world.json HelloWorld "Hello world!" greeting.json
solana evm hello-world.json update "Goodbye cruel world!" greeting.json
```

### Contract to account mapping

A Solana instruction includes a list of public keys corresponding to any account
it references. The EVM then receives an ordered key-value store, mapping those
public keys to accounts, which we'll call *account store*. Because the account store
is ordered, it can be used as a stack. Because it includes the public keys, it can
be used as a heap.

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
`EvmInstruction::InitializeContract` that contains a serialized string in
`Instruction.data`.

When the EVM starts executing the `HelloWorld` constructor, it uses the first
account from the account store to host the contents of `initMessage`. It then
uses the second account to host all member variables. For `HelloWorld`, that
second account holds a hash of the contract name, `"HelloWorld"`, and the
address to the first account in its `message` field.

When the `update()` function is called, the EVM reallocates memory for the
message account, as-needed, and copies `newMessage` into it.

### Runtime data as sysvars mapping

The EVM has special instructions to access runtime data, such as TIMESTAMP. To
access the timestamp, the EVM will query the account store for
`sysvar::clock::id()`.

### Collecting input account addresses

There are three levels of method complexity that determine how to collect the
list of input account addresses to pass into them:

1. Methods that reference accounts defined by the method signature
2. Methods that reference accounts defined by the method implementation
3. Methods that use account values to determine what accounts it needs next

In the first case, use the ABI file to calculate the account list.

In the second case, the layout of the instruction's address list depends on the
method implementation. To generate the account expectations, you need to
execute the smart contract locally.

In the third case, executing locally will stop early, because the
implementation depends on the value of an account. To get the remaining account
expectations, you need to invoke the method again locally, this time passing in
the first set of accounts. You will need to continue that process until the
transaction completes successfully.

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

### Security risks

* Pre-executing smart contracts to acquire account keys implies the client
  needs to trust the tool producing account keys.
