## Problem

Inter-chain applications are not new to the digital asset ecosystem; in fact, even
the smaller centralized exchanges still categorically dwarf all single chain dapps
put together in terms of users and volume. They command massive valuations and
have spent years effectively optimizing their core products for a broad range of
end users. However, their basic operations center around mechanisms that require
their users to unilaterally trust them, typically with little to no recourse
or protection from accidental loss. This has led to the broader digital asset
ecosystem being fractured along network lines because interoperability solutions typically:
 * Are technically complex to fully implement
 * Create unstable network scale incentive structures
 * Require consistent and high level cooperation between stakeholders


## Proposed Solution

Simple Payment Verification (SPV) is a generic term for a range of different
methodologies used by light clients on most major blockchain networks to verify
aspects of the network state without the burden of fully storing and maintaining
the chain itself. In most cases, this means relying on a form of hash tree to
supply a proof of the presence of a given transaction in a certain block by
comparing against a root hash in that block’s header or equivalent. This allows
a light client or wallet to reach a probabilistic level of certainty about
on-chain events by itself with a minimum of trust required with regard to network nodes.

Traditionally the process of assembling and validating these proofs is carried
out off chain by nodes, wallets, or other clients, but it also offers a potential
mechanism for inter-chain state verification. However, by moving the capability
to validate SPV proofs on-chain as a smart contract while leveraging the archival
properties inherent to the blockchain, it is possible to construct a system for
programmatically detecting and verifying transactions on other networks without
the involvement of any type of trusted oracle or complex multi-stage consensus
mechanism. This concept is broadly generalisable to any network with an SPV
mechanism and can even be operated bilaterally on other smart contract platforms,
opening up the possibility of cheap, fast, inter-chain transfer of value without
relying on collateral, hashlocks, or trusted intermediaries.

Opting to take advantage of well established and developmentally stable mechanisms
already common to all major blockchains allows SPV based interoperability solutions
to be dramatically simpler than orchestrated multi-stage approaches. As part of
this, they dispense with the need for widely agreed upon cross chain communication
standards and the large multi-party organizations that write them in favor of a
set of discrete contract-based services that can be easily utilized by caller
contracts through a common abstraction format. This will set the groundwork for
a broad range of dapps and contracts able to interoperate across the variegated
and every growing platform ecosystem.

## Terminology

SPV Program - Client-facing interface for the inter-chain SPV system, manages participant roles.
SPV Engine - Validates transaction proofs, subset of the SPV Program.
Client  - The caller to the SPV Program, typically another solana contract.
Prover - Party who generates proofs for transactions and submits them to the SPV Program.
Transaction Proof - Created by Provers, contains a merkle proof, transaction, and blockheader reference.
Merkle Proof - Basic SPV proof that validates the presence of a transaction in a certain block.
Block Header - Represents the basic parameters and relative position of a given block.
Proof Request - An order placed by a client for verification of transaction(s) by provers.
Header Store - A data structure for storing and referencing ranges of block headers in proofs.
Client Request - Transaction from the client to the SPV Program to trigger creation of a Proof Request.
Sub-account - A Solana account owned by another contract account, without its own private key.

For more information on the Inter-chain SPV system, see the book section at:
https://solana-labs.github.io/book/interchain-transaction-verification.html
