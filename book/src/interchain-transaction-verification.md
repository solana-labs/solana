
# Inter-chain Transaction Verification Program

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


## Service

SPV Programs run as contracts deployed on the Solana network and maintain a type
of public marketplace for SPV proofs that allows any party to submit both requests
for proofs as well as proofs themselves for verification in response to requests.
There will be multiple SPV Program instances active at any given time, at least
one for each connected external network and potentially multiple instances per
network. SPV program instances will be relatively consistent in their high level
API and feature sets with some variation between currency platforms (Bitcoin,
Litecoin) and smart contract platforms owing to the potential for verification of
network state changes beyond simply transactions. In every case regardless of
network, the SPV Program relies on an internal component called an SPV engine to
provide stateless verification of the actual SPV proofs upon which the higher
level client facing features and api are built. The SPV engine requires a
network specific implementation, but allows easy extension of the larger
inter-chain ecosystem by any team who chooses to carry out that implementation
and drop it into the standard SPV program for deployment.

For purposes of Proof Requests, the requester is referred to as the program client,
which in most if not all cases will be another Solana Contract. The client can
choose to submit a request pertaining to a specific transaction or to include a
broader filter that can apply to any of a range of parameters of a transaction
including its inputs, outputs, and amount. For example, A client could submit a
request for any transaction sent from a given address A to address B with the
amount X after a certain time. This structure can be used in a range of
applications, such as verifying a specific intended payment in the case of an
atomic swap or detecting the movement of collateral assets for a loan.

Following submission of a Client Request, assuming that it is successfully
validated, a proof request account is created by the SPV program to track the
progress of the request. Provers use the account to specify the request they
intend to fill in the proofs they submit for validation, at which point the
SPV program validates those proofs and if successful, saves them to the account
data of the request account. Clients can monitor the status of their requests
and see any applicable transactions alongside their proofs by querying the
account data of the request account. In future iterations when supported by
Solana, this process will be simplified by contracts publishing events rather
than requiring a polling style process as described.

## Implementation

The Solana Inter-chain SPV mechanism consists of the following components and participants:

#### SPV engine
A contract deployed on Solana which statelessly verifies SPV proofs for the caller.
It takes as arguments for validation:
* An SPV proof in the correct format of the blockchain associated with the program
* Reference(s) to the relevant block headers to compare that proof against
* The necessary parameters of the transaction to verify
If the proof in question is successfully validated, the SPV program saves proof
of that verification to the request account, which can be saved by the caller to
its account data or otherwise handled as necessary. SPV programs also expose
utilities and structs used for representation and validation of headers,
transactions, hashes, etc. on a chain by chain basis.

#### SPV program
A contract deployed on Solana which coordinates and intermediates the interaction
between Clients and Provers and manages the validation of requests, headers,
proofs, etc. It is the primary point of access for Client contracts to access the
inter-chain. SPV mechanism. It offers the following core features:
* Submit Proof Request - allows client to place a request for a specific proof or set of proofs
* Cancel Proof Request - allows client to invalidate a pending request
* Fill Proof Request   - used by Provers to submit for validation a proof corresponding to a given Proof Request
The SPV program maintains a publicly available listing of valid pending Proof
Requests in its account data for the benefit of the Provers, who monitor it and
enclose references to target requests with their submitted proofs.


#### Proof Request
A message sent by the Client to the SPV engine denoting a request for a proof of
a specific transaction or set of transactions. Proof Requests can either manually
specify a certain transaction by its hash or can elect to submit a filter that
matches multiple transactions or classes of transactions. For example, a filter
matching “any transaction from address xxx to address yyy” could be used to detect
payment of a debt or settlement of an inter-chain swap. Likewise, a filter matching
“any transaction from address xxx” could be used by a lending or synthetic token
minting contract to monitor and react to changes in collateralization. Proof
Requests are sent with a fee, which is disbursed by the SPV engine contract to
the appropriate Prover once a proof matching that request is validated.

#### Request Book
The public listing of valid, open Proof Requests available to provers to fill or
for clients to cancel. Roughly analogous to an orderbook in an exchange, but with
a single type of listing rather than two separate sides. It is stored in the
account data of the SPV program.

#### Proof
A proof of the presence of a given transaction in the blockchain in question.
Proofs encompass both the actual merkle proof and reference(s) to a chain of valid
sequential block headers. They are constructed and submitted by Provers in
accordance with the specifications of the publicly available Proof Requests
hosted on the request book by the SPV program. Upon Validation, they are saved
to the account data of the relevant Proof Request, which can be used by the
Client to monitor the state of the request.

#### Client
The originator of a request for a transaction proof. Clients will most often be
other contracts as parts of dapps or specific financial products like loans,
swaps, escrow, etc. The client in any given verification process cycle initially
submits a ClientRequest which communicates the parameters and fee and if
successfully validated, results in the creation of a Proof Request account by
the SPV program. The Client may also submit a CancelRequest referencing an active
Proof Request in order to denote it as invalid for purposes of proof submission.

#### Prover
The submitter of a proof that fills a Proof Request. Provers monitor the request
book of the SPV program for outstanding Proof Requests and generate matching
proofs, which they submit to the SPV program for validation. If the proof is
accepted, the fee associated with the Proof Request in question is disbursed to
the Prover. Provers typically operate as Solana Blockstreamer nodes that also
have access to a Bitcoin node, which they use for purposes of constructing proofs
and accessing block headers.

#### Header Store
An account-based data structure used to maintain block headers for the purpose
of inclusion in submitted proofs by reference to the header store account.
header stores can be maintained by independent entities, since header chain
validation is a component of the SPV program proof validation mechanism. Fees
that are paid out by Proof Requests to Provers are split between the submitter
of the merkle proof itself and the header store that is referenced in the
submitted proof. Due to the current inability to grow already allocated account
data capacity, the use case necessitates a data structure that can grow
indefinitely without rebalancing. Sub-accounts are accounts owned by the SPV
program without their own private keys that are used for storage by allocating
blockheaders to their account data. Multiple potential approaches to the
implementation of the header store system are feasible:

Store Headers in program sub-accounts indexed by Public address:
* Each sub-account holds one header and has a public key matching the blockhash
* Requires same number of account data lookups as confirmations per verification
* Limit on number of confirmations (15-20) via max transaction data ceiling
* No network-wide duplication of individual headers

Linked List of multiple sub-accounts storing headers:
* Maintain sequential index of storage accounts, many headers per storage account
* Max 2 account data lookups for >99.9% of verifications (1 for most)
* Compact sequential data address format allows any number of confirmations and fast lookups
* Facilitates network-wide header duplication inefficiencies
