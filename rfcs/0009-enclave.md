# Signing using Secure Enclave

The goal of this RFC is to define the security mechanism of signing keys used by the network nodes. Every node contains an asymmetric key that's used for signing and verifying the transactions (e.g. votes). The node signs the transactions using its private key. Other entities can verify the signature using the node's public key.

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


## Branch determination

Due to the fact that the enclave cannot process PoH, it has no direct knowledge of branch history of a submitted validator vote. Each enclave should be initiated with the current *active set* of public keys. A validator should submit its current vote along with the votes of the active set (including itself) that it observed in the slot of its previous vote. In this way, the enclave can surmise the votes accompanying the validator's previous vote and thus the branch being voted on. This is not possible for the validator's initial submitted vote, as it will not have a 'previous' slot to reference. To account for this, a short voting freeze should apply until the second vote is submitted containing the votes within the active set, along with it's own vote, at the height of the initial vote.

## Enclave configuration

A staking client should be configurable to prevent voting on inactive branches. This mechanism should use the client's known active set `N_active` along with a threshold vote `N_vote` and a threshold depth `N_depth` to determine whether or not to continue voting on a submitted branch. This configuration should take the form of a rule such that the client will only vote on a branch if it observes more than `N_vote` at `N_depth`. Practically, this represents the client from confirming that it has observed some probability of economic finality of the submitted branch at a depth where an additional vote would create a lockout for an undesirable amount of time if that branch turns out not to be live.

## Challenges

1. The nodes are currently being configured with asymmetric keys that are generated and stored in PKCS8 files.
2. The genesis block contains an entry that's signed with leader's private key. This entry is used to identify the primordial leader.
3. Generation of verifiable data in untrusted space for PoH verification in the enclave.
4. Need infrastructure for granting stake to an ephemeral key.
