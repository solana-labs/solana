---
title: Inter-chain Transaction Verification
---

## Goal

Enable inter-chain transactions from Solana to Ethereum by implementing a Solana
light client as an Ethereum smart contract.

## Solana Light Client as an Ethereum Contract

A light client is a cluster participant that can verify the state of a
blockchain without directly running a validator. Instead of storing the entire
state of the blockchain, light clients store the sequence of block headers that
contain Merkle root hashes of the blockchain state. These block headers can be
used by light clients to verify the state of the chain without fully trusting
validator nodes. To verify that a specific transaction is reflected in the
state, for example, a light client can query a validator node and request for a
Merkle inclusion-proof for the transaction in a certain block header.

Light clients enable lightweight devices to participate in a cluster without
fully trusting validator nodes. However, they are also useful for inter-chain
transactions. Consider a Solana light client that is implemented as an Ethereum
smart contract. Then Solana participants can submit Merkle proofs to the
Ethereum network to have the light client smart contract verify specific state
information in the Solana network.

### Example: Inter-Chain Transfers

Suppose that a user wishes to transfer 10 tokens from an `SPL Token` account
`account_solana` in Solana to an `ERC20` account `account_ethereum` in Ethereum.
To do this, it can use the `SPL Token` program to burn 10 tokens from
`account_solana`. Then it computes a Merkle inclusion-proof certifying that this
transaction is reflect in the Solana ledger and sends the proof to an Ethereum
`ERC20` contract. The Ethereum contract can then invoke the light client to
verify the proof and then mint 10 tokens to `account_ethereum`.

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
is indeed a correctly voted block: it verifies the signatures and checks that
the valid signatures constitute a super-majority vote by the validators. Then,
it can verify that `B_{i+N}` is an `N`th subsequent block to `B_i`: it checks
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
keep up (computationally and storage-wise) with the continuous stream of the
block headers. The larger `N` is, the slower an interchain transaction will be
as block information is only verified by an Ethereum contract every `N` slots.

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
a user can just provide `B_{i+j}`, a Merkle inclusion proof for the block header
`B_{i+j}`, and a Merkle inclusion proof for the transaction in `B_{i+j}`.

### Signature Aggregation/Batching

A big bottle neck in verifying block header information in the Ethereum light
client contract will be verifying the validator signatures. To futureproof the
Solana-Ethereum bridge, the light client contract must ideally process up to
10,000 validator node votes. If
[EIP-665](https://eips.ethereum.org/EIPS/eip-665) or
[EIP-1829](https://eips.ethereum.org/EIPS/eip-1829) are implemented, then this
can be done on an Ethereum contract. However, it appears unlikely that these
will be implemented
([discussion](https://threader.app/thread/1296631142817423360)) and therefore,
vote verification cannot be done on an Ethereum contract.

One possible way to allow vote verification on an Ethereum contract is to
upgrade the Solana consensus protocol to utilize an aggregatable signature
scheme like BLS instead of Ed25519. BLS signatures are aggregatable and can be
batch verified: any number of signatures that are generated by different
validators for a single message can be aggregated into a single signature. This
aggregated signature can be verified (almost) at the cost to verify a single
signature. It also appears that BLS signature verification could potentially be
implemented as a pre-compile on Ethereum
[EIP-2537](https://eips.ethereum.org/EIPS/eip-2537).

This optimization, however, requires making change to the current consensus
protocol in Solana. This optimization can be considered for a distant future
upgrade to the Solana network, but it is not an immediate solution.

## ZKP Optimizations

Cryptographic proofs can be used to optimize the Ethereum light client contract.

### Background

Cryptographic proofs can be described very formally over the class of `NP`
languages. However, for intuition, the easiest way to think about cryptographic
proofs is to consider a boolean output function with `common` and `prover_only`
data as input.

```rust

fn verify(common: CommonData, prover_only: ProverOnlyData) -> bool;

```

The `common` data is available to both the prover and the verifier. The
`prover_only` data is available to only the prover.

- Proofs that hide `prover_only` data from the verifier are called
  `zero-knowledge` proofs.
- Proofs that compress `prover_only` data from the verifier are called usually
  called `SNARKs`.
- Proofs that both hide and compress `prover_only` data from the verifier are
  called `ZK-SNARKs`.

### ZK-Proofs

Zero-knowledge proofs are useful if we want to prove statements about encrypted
data without revealing the data itself. Suppose that a prover wants to convince
a verifier that a cryptographic ciphertext encrypts a number between `0` and
`10`. Then we can use a ZKP for the following verification function:

```rust

struct CommonData(Ciphertext);
struct ProverOnlyData {
  decryption_key: DecryptionKey,
  encrypted_number: u64,
}

fn verify(common: CommonData, prover_only: ProverOnlyData) -> bool {
  let ciphertext = common.0;
  let decryption_key = prover_only.decryption_key;
  let encrypted_number = prover_only.encrypted_number;

  if encrypted_number != ciphertext.decrypt(decryption_key) {
    return false;
  }

  if 0 <= encrypted_number <= 10 {
    return false;
  }

  true
}

```

These type of zero-knowledge proofs are also called `range proofs`. A
zero-knowledge proof `zk_proof` for the verification function above with data
`common` certifies that

1. The prover knows a `prover_only` data that makes the verification function
   output `true` for `common`.
2. The proof `zk_proof` does not reveal any information about `prover_only`
   other than what can already be inferred from the fact that `verify(common,
   prover_only) == true`.

### SNARKs

For the light client optimization, we do not seek to hide data. Rather, our goal
is to optimize light client verification performance. SNARKs allow for such
optimization.

```rust

fn verify(common: CommonData, prover_only: ProverOnlyData) -> bool;

```

For any verification function with the syntax above, SNARKs allow a prover to
generate a proof `snark_proof` that certifies that:

1. The prover knows a `prover_only` data that makes the verification function
   output `true` for `common`.
2. The byte-size of `snark_proof` is at most (asymptotically) a log of the
   byte-size of `prover_only` data.
3. The computation needed to verify `snark_proof` is at most (asymptotically) a
   log of the computation needed to evaluate the `verify` function.

### Signatures Verification

One natural way to apply SNARKs for light client verification is to use it to
optimize signature verification.

```rust

struct CommonData(Block);
struct ProverOnlyData {
  pubkeys: Vec<Pubkey>,
  signatures: Vec<Signatures>,
};

fn verify(common: CommonData, prover_only: ProverOnlyData) -> bool {
  let block = common.0;
  let pubkeys = prover_only.pubkeys;
  let signatures = prover_only.signatures;

  for (pubkey, signature) in pubkeys.iter().zip(signatures.iter()) {
    if !Ed25519::verify(pubkey, block, signature) {
      return false;
    }
  }
  true
}

```

SNARKs guarantees that the time to verify the proof requires computation at most
a log of the computation needed to verify all the signatures in a vote. Allowing
the light client to verify the SNARK proof as opposed to the individual
signatures, can asymptotically decrease the cost of the light client.

### Merkle Root Verification

Another way to apply SNARKs for light client verification is to use it to
optimize Merkle Tree computation.

```rust

struct CommonData(MerkleRoot);
struct ProverOnlyData(Vec<Blocks>);

fn verify(common: CommonData, prover_only: ProverOnlyData) -> bool {
  let root = common.0;
  let blocks = prover_only.0;

  if root != MerkleRoot::create(blocks) {
    return false;
  }
  true
}

```

Instead of submitting a list of blocks to the Ethereum contract for it to
compute their Merkle root, the prover can compute the Merkle root itself and
then submit the root along with a SNARK proof that the root was generated
correctly. The Ethereum contract verifies the SNARK proof and simply stores the
root that was sent by the prover.

### Recursive Composition

(TODO: Discuss recursive composition)
