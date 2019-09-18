# Secure Vote Signing

A validator fullnode receives entries from the current leader and submits votes confirming those entries are valid. This vote submission presents a security challenge, because forged votes that violate consensus rules could be used to slash the validator's stake.

The validator votes on its chosen fork by submitting a transaction that uses an asymmetric key to sign the result of its validation work. Other entities can verify this signature using the validator's public key. If the validator's key is used to sign incorrect data \(e.g. votes on multiple forks of the ledger\), the node's stake or its resources could be compromised.

Solana addresses this risk by splitting off a separate _vote signer_ service that evaluates each vote to ensure it does not violate a slashing condition.

## Validators, Vote Signers, and Stakeholders

When a validator receives multiple blocks for the same slot, it tracks all possible forks until it can determine a "best" one. A validator selects the best fork by submitting a vote to it, using a vote signer to minimize the possibility of its vote inadvertently violating a consensus rule and getting a stake slashed.

A vote signer evaluates the vote proposed by the validator and signs the vote only if it does not violate a slashing condition. A vote signer only needs to maintain minimal state regarding the votes it signed and the votes signed by the rest of the cluster. It doesn't need to process a full set of transactions.

A stakeholder is an identity that has control of the staked capital. The stakeholder can delegate its stake to the vote signer. Once a stake is delegated, the vote signer votes represent the voting weight of all the delegated stakes, and produce rewards for all the delegated stakes.

Currently, there is a 1:1 relationship between validators and vote signers, and stakeholders delegate their entire stake to a single vote signer.

## Signing service

The vote signing service consists of a JSON RPC server and a request processor. At startup, the service starts the RPC server at a configured port and waits for validator requests. It expects the following type of requests: 1. Register a new validator node

* The request must contain validator's identity \(public key\)
* The request must be signed with the validator's private key
* The service drops the request if signature of the request cannot be

  verified

* The service creates a new voting asymmetric key for the validator, and

  returns the public key as a response

* If a validator tries to register again, the service returns the public key

  from the pre-existing keypair

  1. Sign a vote

* The request must contain a voting transaction and all verification data
* The request must be signed with the validator's private key
* The service drops the request if signature of the request cannot be

  verified

* The service verifies the voting data
* The service returns a signature for the transaction

## Validator voting

A validator node, at startup, creates a new vote account and registers it with the cluster by submitting a new "vote register" transaction. The other nodes on the cluster process this transaction and include the new validator in the active set. Subsequently, the validator submits a "new vote" transaction signed with the validator's voting private key on each voting event.

### Configuration

The validator node is configured with the signing service's network endpoint \(IP/Port\).

### Registration

At startup, the validator registers itself with its signing service using JSON RPC. The RPC call returns the voting public key for the validator node. The validator creates a new "vote register" transaction including this public key, and submits it to the cluster.

### Vote Collection

The validator looks up the votes submitted by all the nodes in the cluster for the last voting period. This information is submitted to the signing service with a new vote signing request.

### New Vote Signing

The validator creates a "new vote" transaction and sends it to the signing service using JSON RPC. The RPC request also includes the vote verification data. On success, the RPC call returns the signature for the vote. On failure, RPC call returns the failure code.

