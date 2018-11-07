# Signing using Secure Enclave

The goal of this RFC is to define the security mechanism of signing keys used by the network nodes. Every node contains an asymmetric key that's used for signing and verifying the transactions (e.g. votes). The node signs the transcations using its private key. Other entities can verify the signature using the node's public key.

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

## Challenges

1. The nodes are currently being configured with asymmetric keys that are generated and stored in PKCS8 files.
2. The genesis block contains an entry that's signed with leader's private key. This entry is used to identify the primordial leader.
3. Generation of verifiable data in untrusted space for PoH verification in the enclave.
4. Need infrastructure for granting stake to an ephemeral key.
