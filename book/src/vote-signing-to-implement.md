# Secure Vote Signing

This design describes additional vote signing behavior that will make the
process more secure.

Currently, Solana implements a vote-signing service that evaluates each vote to
ensure it does not violate a slashing condition. The service could potentially
have different variations, depending on the hardware platform capabilities. In
particular, it could be used in conjunction with a secure enclave (such as SGX).
The enclave could generate an asymmetric key, exposing an API for user
(untrusted) code to sign the vote transactions, while keeping the vote-signing
private key in its protected memory.

The following sections outline how this architecture would work:

## Message Flow

1. The node initializes the enclave at startup
    * The enclave generates an asymmetric key and returns the public key to the
      node
    * The keypair is ephemeral. A new keypair is generated on node bootup. A
      new keypair might also be generated at runtime based on some TBD
      criteria.
    * The enclave returns its attestation report to the node
2. The node performs attestation of the enclave (e.g using Intel's IAS APIs)
    * The node ensures that the Secure Enclave is running on a TPM and is
      signed by a trusted party
3. The stakeholder of the node grants ephemeral key permission to use its stake.
   This process is TBD.
4. The node's untrusted, non-enclave software calls trusted enclave software
   using its interface to sign transactions and other data.
    * In case of vote signing, the node needs to verify the PoH. The PoH
     verification is an integral part of signing. The enclave would be
     presented with some verifiable data to check before signing the vote.
    * The process of generating the verifiable data in untrusted space is TBD

## PoH Verification

1. When the node votes on an en entry `X`, there's a lockout period `N`, for
which it cannot vote on a fork that does not contain `X` in its history.
2. Every time the node votes on the derivative of `X`, say `X+y`, the lockout
period for `X` increases by a factor `F` (i.e. the duration node cannot vote on
a fork that does not contain `X` increases).
    * The lockout period for `X+y` is still `N` until the node votes again.
3. The lockout period increment is capped (e.g. factor `F` applies maximum 32
times).
4. The signing enclave must not sign a vote that violates this policy. This
means
    * Enclave is initialized with `N`, `F` and `Factor cap`
    * Enclave stores `Factor cap` number of entry IDs on which the node had
      previously voted
    * The sign request contains the entry ID for the new vote
    * Enclave verifies that new vote's entry ID is on the correct fork
      (following the rules #1 and #2 above)

## Ancestor Verification

This is alternate, albeit, less certain approach to verifying voting fork.
1. The validator maintains an active set of nodes in the cluster
2. It observes the votes from the active set in the last voting period
3. It stores the ancestor/last_tick at which each node voted
4. It sends new vote request to vote-signing service
    * It includes previous votes from nodes in the active set, and their
      corresponding ancestors
5. The signer checks if the previous votes contains a vote from the validator,
and the vote ancestor matches with majority of the nodes
    * It signs the new vote if the check is successful
    * It asserts (raises an alarm of some sort) if the check is unsuccessful

The premise is that the validator can be spoofed at most once to vote on
incorrect data. If someone hijacks the validator and submits a vote request for
bogus data, that vote will not be included in the PoH (as it'll be rejected by
the cluster). The next time the validator sends a request to sign the vote, the
signing service will detect that validator's last vote is missing (as part of
#5 above).

## Fork determination

Due to the fact that the enclave cannot process PoH, it has no direct knowledge
of fork history of a submitted validator vote. Each enclave should be initiated
with the current *active set* of public keys. A validator should submit its
current vote along with the votes of the active set (including itself) that it
observed in the slot of its previous vote. In this way, the enclave can surmise
the votes accompanying the validator's previous vote and thus the fork being
voted on. This is not possible for the validator's initial submitted vote, as
it will not have a 'previous' slot to reference. To account for this, a short
voting freeze should apply until the second vote is submitted containing the
votes within the active set, along with it's own vote, at the height of the
initial vote.

## Enclave configuration

A staking client should be configurable to prevent voting on inactive forks.
This mechanism should use the client's known active set `N_active` along with a
threshold vote `N_vote` and a threshold depth `N_depth` to determine whether or
not to continue voting on a submitted fork. This configuration should take the
form of a rule such that the client will only vote on a fork if it observes
more than `N_vote` at `N_depth`. Practically, this represents the client from
confirming that it has observed some probability of economic finality of the
submitted fork at a depth where an additional vote would create a lockout for
an undesirable amount of time if that fork turns out not to be live.

## Challenges

1. Generation of verifiable data in untrusted space for PoH verification in the
   enclave.
2. Need infrastructure for granting stake to an ephemeral key.
