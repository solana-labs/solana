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

### Contract to account mapping

### Runtime data as sysvars mapping

### Collecting account addresses

### Going gasless

### Calling Solidity methods from Solana programs

### Executing Solana instructions from Solidity methods

### What about long-running smart contracts

Not supported by the gasless EVM. Instead, the smart contract would need to be
executed on a traditional EVM, which is outside the scope of this proposal.

### Technical risks

* 256-bit EVM instructions may hit the BPF instruction limit so quickly that
  only the smallest of Solidity smart contracts would work
* Solana's UDP packets must stay under the MTU limit, 1,500 bytes. Complex
  Solidity contracts may implicitly reference many accounts and therefore
  exceed that limit.

### Security risks

* Pre-executing smart contracts to acquire account keys implies the client
  needs to trust the tool producing account keys.
