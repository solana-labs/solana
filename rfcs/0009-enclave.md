# Signing using Secure Enclave

The goal of this RFC is to define the security mechanism of signing keys used by the network nodes. Every node contains an asymmetric key that's used for signing and verifying the votes. The node signs the vote transactions using its private key. Other entities can verify the signature using the node's public key.

The node's stake or its resources could be compromised if its private key is used to sign incorrect data (e.g. voting on multiple forks of the ledger). So, it's important to safeguard the private key.

Secure Enclaves (such as SGX) provide a layer of memory and computation protection. An enclave can be used to generate an asymmetric key and keep the private key in its protected memory. It can expose an API that user (untrusted) code can use for signing the transactions.

## Message Flow

1. The node initializes the enclave at startup
    * The enclave generates an asymmetric key and returns the public key to the node
    * The keypair is ephemeral. A new keypair is generated on node bootup. A new keypair might also be generated at runtime based on some TBD criteria.
    * The enclave returns its attestation report to the node
2. The node performs attestation of the enclave (e.g using Intel's IAS APIs)
    * The node ensures that the Secure Enclave is running on a TPM and is signed by a trusted party
3. The owner of the node grants ephemeral key permission to use its stake. This process is TBD.
4. The node's untrusted, non-enclave software calls trusted enclave software using its interface to sign transactions and other data.
    * In case of vote signing, the node needs to verify the PoH. The PoH verification is an integral part of signing. The enclave would be presented with some verifiable data that it'll check before signing the vote.
    * The process of generating the verifiable data in untrusted space is TBD

## PoH Verification

1. When the node votes on an en entry `X`, there's a lockout period `N`, for which it cannot vote on a branch that does not contain `X` in its history.
2. Every time the node votes on the derivative of `X`, say `X+y`, the lockout period for `X` increases by a factor `F` (i.e. the duration node cannot vote on a branch that does not contain `X` increases).
    * The lockout period for `X+y` is still `N` until the node votes again.
3. The lockout period increment is capped (e.g. factor `F` applies maximum 32 times).
4. The signing enclave must not sign a vote that violates this policy. This means
    * Enclave is initialized with `N`, `F` and `Factor cap`
    * Enclave stores `Factor cap` number of entry IDs on which the node had previously voted
    * The sign request contains the entry ID for the new vote
    * Enclave verifies that new vote's entry ID is on the correct branch (following the rules #1 and #2 above)

## Ancestor Verification

This is alternate, albeit, less certain approach to verifying voting branch.
1. The validator maintains an active set of nodes in the network
2. It observes the votes from the active set in the last voting period
3. It stores the ancestor/last_tick at which each node voted
4. It sends new vote request to vote-signing service
    * It includes previous votes from nodes in the active set, and their corresponding ancestors
5. The signer checks if the previous votes contains a vote from the validator, and the vote ancestor matches with majority of the nodes
    * It signs the new vote if the check is successful
    * It asserts (raises an alarm of some sort) if the check is unsuccessful

The premise is that the validator can be spoofed at most once to vote on incorrect data. If someone hijacks the validator and submits a vote request for bogus data, that vote will not be included in the PoH (as it'll be rejected by the network). The next time the validator sends a request to sign the vote, the signing service will detect that validator's last vote is missing (as part of #5 above).

## Branch determination

Due to the fact that the enclave cannot process PoH, it has no direct knowledge of branch history of a submitted validator vote. Each enclave should be initiated with the current *active set* of public keys. A validator should submit its current vote along with the votes of the active set (including itself) that it observed in the slot of its previous vote. In this way, the enclave can surmise the votes accompanying the validator's previous vote and thus the branch being voted on. This is not possible for the validator's initial submitted vote, as it will not have a 'previous' slot to reference. To account for this, a short voting freeze should apply until the second vote is submitted containing the votes within the active set, along with it's own vote, at the height of the initial vote.

## Enclave configuration

A staking client should be configurable to prevent voting on inactive branches. This mechanism should use the client's known active set `N_active` along with a threshold vote `N_vote` and a threshold depth `N_depth` to determine whether or not to continue voting on a submitted branch. This configuration should take the form of a rule such that the client will only vote on a branch if it observes more than `N_vote` at `N_depth`. Practically, this represents the client from confirming that it has observed some probability of economic finality of the submitted branch at a depth where an additional vote would create a lockout for an undesirable amount of time if that branch turns out not to be live.

## Signing service

The signing service consists of a a JSONRPC server, and a request processor. At startup, it starts the RPC server at a configured port and waits for client/validator requests. It expects the following type of requests.
1. Register a new validator node
    * The request contains validator's identity (public key)
    * The request is signed with validator's private key
    * The service will drop the request if signature of the request cannot be verified
    * The service will create a new voting asymmetric key for the validator, and return the public key as a response
    * If a validator retries to register, it'll return the public key from the pre-existing keypair
2. Sign a vote
    * The request contains voting transaction, and all verification data (as described in Ancestor Verification)
    * The request is signed with validator's private key
    * The service will drop the request if signature of the request cannot be verified
    * The service will verify the voting data
    * The service will return a signed transaction (or signature for the transaction)

The service could potentially have different variations, depending on the hardware platform capabilities. For example, if the hardware supports a secure enclave, the service can offload asymmetric key generation, and private key protection to the enclave. A less secure implementation of the service could simply carry the keypair in the process memory.

## Validator voting

A validator node, at startup, creates a new vote account and registers it with the network. This is done by submitting a new "vote register" transaction. The transaction contains validator's keypair, it's vote signing public key, and some additional information. The other nodes on the network process this transaction and include the new validator in the active set.

Subsequently, the validator submits a "new vote" transaction on a voting event. This vote is signed with validator's voting private key.

The validator code will change to interface with Signing service for "vote register" and "new vote" use cases.

### Configuration

The validator node will be configured with Signing service's network endpoint (IP/Port).

### Register

At startup, the validator will call Signing service using JSON RPC to register itself. The RPC call will return the voting public key for the validator node. The validator will create a new "vote register" transaction including this public key in it, and submit it to the network.

### Collect votes for last period

The validator will look up the votes submitted by all the nodes in the network for the last voting period. This information will be submitted to signing service with new vote signing request.

### New Vote Signing

The validator will create a "new vote" transaction and send it to the signing service using JSON RPC. The RPC request will also include the vote verification data. On success, RPC call will return the signature for the vote. On failure, RPC call will return the failure code.

## Challenges

1. The nodes are currently being configured with asymmetric keys that are generated and stored in PKCS8 files.
2. The genesis block contains an entry that's signed with leader's private key. This entry is used to identify the primordial leader.
3. Generation of verifiable data in untrusted space for PoH verification in the enclave.
4. Need infrastructure for granting stake to an ephemeral key.
