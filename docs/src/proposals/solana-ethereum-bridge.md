---
title: Inter-chain Transaction Verification
---

## Goal

## Solana Light Client as an Ethereum Contract

A light client is a cluster participant that can verify the state of a
blockchain without directly running a validator. Instead of storing the entire
state of the blockchain, light clients store the sequence of block headers that
contain Merkle root hashes of the blockchain state. These block headers can be
used by light clients to verify the state of the chain without fully trusting
validator nodes. To verify that a specific transaction is reflected in the
state, for example, a light client can query a validator node and request for a
Merkle inclusion-proof for the transaction in a certain block header.

Light clients are useful for lightweight devices to participate in a cluster
without fully trusting validator nodes. However, they are also useful for
inter-chain transactions. Consider a Solana light client that is implemented as
an Ethereum smart contract. Then Solana participants can submit Merkle proofs to
the Ethereum network to have the light client contract verify the proofs.

### Example: Inter-Chain Transfers

Suppose that a user wishes to transfer 10 tokens from an `SPL Token` account
`account_solana` in Solana to an `ERC20` account `account_ethereum` in Ethereum.
To do this, it can use the `SPL Token` program to burn 10 tokens from
`account_solana`. Then it submits a Merkle inclusion-proof that this transaction
is reflected in the Solana ledger to an Ethereum `ERC20` contract. The Ethereum
contract can then invoke the light client to verify the proof and then mint 10
tokens to `account_ethereum`.

## Maintaining Block Headers

The main challenge in running a light client as an Ethereum smart contract is
maintaining the block headers in the contract. In contrast to a real light
client, it will be too costly to store every block header in an Ethereum smart
contract. Instead, the contract can store a block header for every `N` slots in
the Solana network.

Suppose that there is a service that feeds Solana state information to the
Ethereum smart contract at the end of every `N` slots. More specifically, let
`B_i` be the last block header that was verified by the Ethereum contract. Then
the service provides the following information to the Ethereum network:

1. The block header at the end of the pre-specified slot `B_{i+N}`
2. The validator votes (signatures) for `B_{i+N}`
3. The intermediate block headers `B_{i+1}`, ..., `B_{i+N-1}`

Upon receiving this information, the Ethereum contract can verify that `B_{i+N}`
is indeed a correctly voted block by verifying the signatures and checking that
the valid signatures constitute a super-majority vote by the validators. Then,
it can verify that `B_{i+N}` is an `N`th subsequent block to `B_i` by checking
that each block header `H_j` contains the hash `H(B_{j-1})` for `j = 1, ..., N`.
If these checks pass, then the contract can store the block information `B_{i+N}`.

Even if the Ethereum contract only stores block headers for every `N` slots,
Solana state information can still be verified on Ethereum. For example, suppose
that a user wishes to verify that a transaction that was included in a block
`B_{i+j}`. Then, it can submit the block hashes `B_{i+j}`, `B_{i+j+1}`, ...,
`B_{i+j+N}`, and the Merkle inclusion proof for the root in `B_{i+j}`.

### Validator Stake Information

In order to execute the idea above, the Ethereum contract must maintain the
following state:

1. The block headers for every `N` slots: `B_0`, `B_N`, `B_{2N}`, ...
2. The most recent validator stake information

(TODO: More info about maintaining validator stake information)

## Non-ZKP Optimizations

The efficiency of the method above depends on the number of slot parameter `N`.
The smaller `N` is, the harder it will be for the Ethereum smart contract to
keep up (with respect to both computation and space) with the continuous stream
of the block headers. The larger `N` is, the slower an interchain transaction
will be as block information is only verified on the Ethereum side every `N`
slots.

The performance of the light client can be improved using the following
optimizations.

### Merkle Tree

As specified above, only block headers `B_0`, `B_N`, `B_{2N}`, ... are stored on
the Ethereum contract and therefore, to verify a transaction in a block
`B_{i+j}`, one must provide all the intermediate blocks `B_{i+j}`, `B_{i+j+1}`,
..., `B_{i+j+N}` along with a Merkle inclusion proof that the transaction is
reflected in `B_{i+j}`. One optimization is to change the Ethereum light client
contract to additionally store the Merkle root of all the received block
headers.

Let `B_i` be the most recently verified block header. Then, by the specification
above, the contract receives the following information every `N` slots in the
Solana network:

1. The block header at the end of the pre-specified slot `B_{i+N}`
2. The validator votes (signatures) for `B_{i+N}`
3. The intermediate block headers `B_{i+1}`, ..., `B_{i+N-1}`

After verifying the validator votes and the intermediate block headers, the
contract can additionally compute a Merkle tree of the intermediate block
headers `B_{i+1}`, ..., `B_{i+N-1}` and then store its root in the contract.

Now, to make an interchain transaction in a block `B_{i+j}`, instead of
providing all the intermediate blocks `B_{i+j}`, `B_{i+j+1}`, ..., `B_{i+j+N}`,
it can just provide `B_{i+j}`, a Merkle inclusion proof for the block header
`B_{i+j}`, and a Merkle inclusion proof for the transaction in `B_{i+j}`.

### Signature Aggregation/Batching

A big bottle neck in verifying block header information in the Ethereum light
client contract will be verifying the validator signatures. To futureproof the
Solana-Ethereum bridge, the light client contract must ideally be able to
process 10,000 validator node votes. Verifying this many Ed25519 signatures on
an Ethereum contract will be impractical no matter how large the parameter `N`
is set.

One possible way to make the signature verification more efficient is to use an
aggregatable signature scheme like BLS instead of Ed25519. BLS signatures can be
batch verified in that any number of signatures that are generated by different
authorities but for a same message can be aggregated into a single signature.
This aggregated signature can be verified (almost) at the cost to verify a
single signature.

This optimization is not ideal as it requires making real change to the current
consensus protocol, but it is a possible optimization.

## ZKP Optimizations

(TODO: Discuss optimizations using SNARKs)

### Signatures Verification

### Merkle Root Verification

### Recursive Composition
