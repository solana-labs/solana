# Ledger Replication

## Background

At full capacity on a 1gbps network Solana would generate 4 petabytes of data
per year. If each fullnode was required to store the full ledger, the cost of
storage would discourage fullnode participation, thus centralizing the network
around those that could afford it. Solana aims to keep the cost of a fullnode
below $5,000 USD to maximize participation. To achieve that, the network needs
to minimize redundant storage while at the same time ensuring the validity and
availability of each copy.

To trust storage of ledger segments, Solana has *replicators* periodically
submit proofs to the network that the data was replicated. Each proof is called
a Proof of Replication.  The basic idea of it is to encrypt a dataset with a
public symmetric key and then hash the encrypted dataset. Solana uses [CBC
encryption](https://en.wikipedia.org/wiki/Block_cipher_mode_of_operation#Cipher_Block_Chaining_(CBC)).
To prevent a malicious replicator from deleting the data as soon as it's
hashed, a replicator is required hash random segments of the dataset.
Alternatively, Solana could require hashing the reverse of the encrypted data,
but random sampling is sufficient and much faster.  Either solution ensures
that all the data is present during the generation of the proof and also
requires the validator to have the entirety of the encrypted data present for
verification of every proof of every identity. The space required to validate
is:

``` number_of_proofs * data_size ```

# Optimization with PoH

Solana is not the only distribute systems project using Proof of Replication,
but it might be the most efficient implementation because of its ability to
synchronize nodes with its Proof of History. With PoH, Solana is able to record
a hash of the PoRep samples in the ledger.  Thus the blocks stay in the exact
same order for every PoRep and verification can stream the data and verify all
the proofs in a single batch.  This way Solana can verify multiple proofs
concurrently, each one on its own GPU core. With the current generation of
graphics cards our network can support up to 14,000 replication identities or
symmetric keys. The total space required for verification is:

``` 2 CBC_blocks * number_of_identities ```

with core count of equal to (Number of Identities). A CBC block is expected to
be 1MB in size.

# Network

Validators for PoRep are the same validators that are verifying transactions.
They have some stake that they have put up as collateral that ensures that
their work is honest. If you can prove that a validator verified a fake PoRep,
then the validator's stake is slashed.

Replicators are specialized light clients. They download a part of the ledger
and store it and provide proofs of storing the ledger. For each verified proof,
replicators are rewarded tokens from the mining pool.

# Constraints

Solana's PoRep protocol instroduces the following constraints:

* At most 14,000 replication identities can be used, because that is how many GPU
  cores are currently available to a computer costing under $5,000 USD.
* Verification requires generating the CBC blocks. That requires space of 2
  blocks per identity, and 1 GPU core per identity for the same dataset. As
many identities at once are batched with as many proofs for those identities
verified concurrently for the same dataset.

# Validation and Replication Protocol

1. The network sets a replication target number, let's say 1k. 1k PoRep
   identities are created from signatures of a PoH hash. They are tied to a
specific PoH hash. It doesn't matter who creates them, or it could simply be
the last 1k validation signatures we saw for the ledger at that count. This is
maybe just the initial batch of identities, because we want to stagger identity
rotation.
2. Any client can use any of these identities to create PoRep proofs.
   Replicator identities are the CBC encryption keys.
3. Periodically at a specific PoH count, a replicator that wants to create
   PoRep proofs signs the PoH hash at that count. That signature is the seed
used to pick the block and identity to replicate. A block is 1TB of ledger.
4. Periodically at a specific PoH count, a replicator submits PoRep proofs for
   their selected block. A signature of the PoH hash at that count is the seed
used to sample the 1TB encrypted block, and hash it. This is done faster than
it takes to encrypt the 1TB block with the original identity.
5. Replicators must submit some number of fake proofs, which they can prove to
   be fake by providing the seed for the hash result.
6. Periodically at a specific PoH count, validators sign the hash and use the
   signature to select the 1TB block that they need to validate. They batch all
the identities and proofs and submit approval for all the verified ones.
7. After #6, replicator client submit the proofs of fake proofs.

For any random seed, Solana requires everyone to use a signature that is
derived from a PoH hash. Every node uses the same count so that the same PoH
hash is signed by every participant. The signatures are then each
cryptographically tied to the keypair, which prevents a leader from grinding on
the resulting value for more than 1 identity.

Key rotation is *staggered*. Once going, the next identity is generated by
hashing itself with a PoH hash.

Since there are many more client identities then encryption identities, the
reward is split amont multiple clients to prevent Sybil attacks from generating
many clients to acquire the same block of data. To remain BFT, the network
needs to avoid a single human entity from storing all the replications of a
single chunk of the ledger.

Solana's solution to this is to require clients to continue using the same
identity. If the first round is used to acquire the same block for many client
identities, the second round for the same client identities will require a
redistribution of the signatures, and therefore PoRep identities and blocks.
Thus to get a reward for storage, clients are not rewarded for storage of the
first block. The network rewards long-lived client identities more than new
ones.

