
# Bitcoin Payment Verification Program

## Premise

Simple Payment Verification (SPV) is a generic term for a range of different methodologies used by light clients on most major blockchain networks to verify aspects of the network state without the burden of fully storing and maintaining the chain itself. In most cases, this means relying on a form of hash tree to supply a proof of the presence of a given transaction in a certain block by comparing against the merkle root of that block’s header or equivalent structure. This proof enables the low information party to achieve a probabilistically definable level of certainty...

Traditionally the process of assembling and validating these proofs is carried out off chain by  nodes, wallets, or other clients, but it also offers a potential mechanism for inter-chain state verification. By moving the capability to validate SPV proofs on chain as a smart contract while leveraging the archival properties inherent to the blockchain, it is possible to construct a system for programmatically detecting and verifying transactions on other networks without the involvement of any type of oracle or complex multi-stage consensus mechanism. This concept is broadly generalisable to any network with an SPV mechanism and can even be operated bilaterally on other smart contract networks, opening up the possibility of cheap, fast, interchain transfer of value without relying on collateral, hashlocks, or trusted intermediaries. 

Opting to take advantage of well established and developmentally stable mechanisms already common to all major blockchains allows SPV based interoperability solutions to be dramatically simpler than orchestrated multi-stage approaches. As part of this, they dispense with the need for widely agreed upon cross chain communication standards and the large multi-party organizations that write them in favor of a set of discrete contract-based services that can be easily utilized by caller contracts through a common abstraction format. This will set the groundwork for a broad range of dapps and contracts able to interoperate across the variegated and every growing platform ecosystem.


## Implementation

The Solana Interchain SPV mechanism consists of the following components and participants:

#### SPV-Engine
A contract deployed on Solana which statelessly verifies SPV proofs for the caller. It takes as arguments for validation:
An spv proof in the correct format of the blockchain associated with the program
Reference(s) to the relevant block headers to compare that proof against
If the proof in question is successfully validated, the spv-program returns a validation token, which can be saved by the caller to its account data or otherwise handled as necessary.
Spv programs also expose utilities and structs used for representation and validation of headers, transactions, hashes, etc. on a chain by chain basis.

#### SPV-Program
A contract deployed on Solana which coordinates and intermediates the interaction between Clients and Provers and manages the validation of requests, headers, proofs, etc. It is the primary point of access for Client contracts to access the Interchain SPV mechanism. It offers the following core features:
Submit Proof Request - allows client to place a request for a specific proof or set of proofs
Cancel Proof Request - allows client to invalidate a pending request
Fill Proof Request - used by Provers to submit for validation a proof corresponding to a given Proof Request
The SPV-Engine maintains a publicly available listing of valid pending Proof Requests for the benefit of the Provers, who monitor it and enclose references to target requests with their submitted proofs.


#### Proof Request
A message sent by the Client to the Spv-Engine denoting a request for a proof of a specific transaction or set of transactions. Proof Requests can either manually specify a certain transaction by its hash or can elect to submit a filter that matches multiple transactions or classes of transactions. For example, a filter matching “any transaction from address xxx to address yyy” could be used to detect payment of a debt or settlement of an interchain swap. Likewise, a filter matching “any transaction from address xxx” could be used by a lending or synthetic token minting contract to monitor and react to changes in collateralization. Proof Requests are sent with a fee, which is disbursed by the SPV-Engine contract to the appropriate Prover once a proof matching that request is validated.

#### Request Book
The public listing of valid, open Proof Requests available to provers to fill or for clients to cancel. Roughly analogous to an orderbook in an exchange, but with a single type of listing rather than two separate sides. It is stored in the account data of the spv-program.

#### Proof
A proof of the presence of a given transaction in the blockchain in question. Proofs encompass both the actual merkle proof and reference(s) to a chain of valid sequential block headers. They are constructed and submitted by Provers in accordance with the specifications of the publicly available Proof Requests hosted on the request book by the Spv-Program. Upon Validation, they are saved to the account data of the relevant Proof Request, which can be used by the Client to monitor the state of the request.

#### Client
The originator of a request for a transaction proof. Clients will most often be other contracts as parts of dapps or specific financial products like loans, swaps, escrow, etc. The client in any given verification process cycle initially submits a ClientRequest which communicates the parameters and fee and if successfully validated, results in the creation of a Proof Request account by the spv-program. The Client may also submit a CancelRequest referencing an active Proof Request in order to denote it as invalid for purposes of proof submission. 

#### Prover
The submitter of a proof that fills a Proof Request. Provers monitor the request book of the spv-program for outstanding Proof Requests and generate matching proofs, which they submit to the spv-program for validation. If the proof is accepted, the fee associated with the Proof Request in question is disbursed to the Prover.
Provers typically operate as solana Blockstreamer nodes that also have access to a Bitcoin node, which they use for purposes of constructing proofs and accessing block headers. 

#### HeaderStore
An account-based data structure used to maintain block headers for the purpose of inclusion in submitted proofs by reference to the HeaderStore account. HeaderStores can be maintained by independent entities, since header chain validation is a component of the spv-program proof validation mechanism. Multiple potential approaches to the implementation of the HeaderStore system are feasible:
Single sub-account per header
Account pubkey is the header blockhash
Requires same number of account data lookups as confirmations per verification
Limit on number of confirmations (15-20) via max transaction data ceiling 
No network-wide duplication of individual headers
Linked List of multiple sub-accounts
Maintain sequential index of storage accounts, many headers per account
Max 2 account data lookups for >99% of verifications (1 for most)
Compact sequential data address format allows any number of confirmations
Facilitates network-wide header duplication inefficiencies
