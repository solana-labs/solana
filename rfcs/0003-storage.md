# Leader Replication

At full capacity on a 1gbps network solana will generate 4 petabytes of data
per year.  To prevent the network from centralizing around full nodes that have
to store the full data set this protocol proposes a way for mining nodes to
provide storage capacity for pieces of the network.

The basic idea to Proof of Replication is encrypting a dataset with a public
symmetric key using CBC encryption, then hash the encrypted dataset. The main
problem with the naive approach is that a dishonest storage node can stream the
encryption and delete the data as its hashed. The simple solution is to force
the hash to be done on the reverse of the encryption, or perhaps with a random
order. This ensures that all the data is present during the generation of the
proof and it also requires the validator to have the entirety of the encrypted
data present for verification of every proof of every identity. So the space
required to validate is `number_of_proofs * data_size`

## Definitions

#### replicator

Storage mining client, stores some part of the ledger enumerated in blocks and
submits storage proofs to the chain. Not a full-node.

#### ledger segment

Portion of the ledger which is downloaded by the replicator where storage proof
data is derived.

#### CBC block

Smallest encrypted chunk of ledger, an encrypted ledger segment would be made of
many CBC blocks. `ledger_segment_size / cbc_block_size` to be exact.

#### storage proof

A set of sha hash state which is constructed by sampling the encrypted version
of the stored ledger segment at certain offsets.

#### fake storage proof

A proof which has the same format as a storage proof, but the sha state is
actually from hashing a known ledger value which the storage client can reveal
and is also easily verifiable by the network on-chain.

#### storage proof confirmation

A transaction by a validator which indicates the set of real and fake proofs
submitted by a storage miner. The transaction would contain a list of proof
hash values and a bit which says if this hash is valid or fake.

#### storage proof challenge

A transaction from a replicator that verifiably proves that a validator
confirmed a fake proof.

#### storage proof claim

A transaction from a validator which is after the timeout period given from the
storage proof confirmation and which no successful challenges have been
observed which rewards the parties of the storage proofs and confirmations.

#### storage validation capacity

The number of keys and samples that a validator can verify each storage epoch.

## Optimization with PoH

Our improvement on this approach is to randomly sample the encrypted segments
faster than it takes to encrypt, and record the hash of those samples into the
PoH ledger. Thus the segments stay in the exact same order for every PoRep and
verification can stream the data and verify all the proofs in a single batch.
This way we can verify multiple proofs concurrently, each one on its own CUDA
core. The total space required for verification is `1_ledger_segment +
2_cbc_blocks * number_of_identities` with core count of equal to
`number_of_identities`. We use a 64-byte chacha CBC block size.

## Network

Validators for PoRep are the same validators that are verifying transactions.
They have some stake that they have put up as collateral that ensures that
their work is honest. If you can prove that a validator verified a fake PoRep,
then the validators stake can be slashed.

Replicators are specialized thin clients. They download a part of the ledger
and store it, and provide PoReps of storing the ledger. For each verified PoRep
replicators earn a reward of sol from the mining pool.

## Constraints

We have the following constraints:
* Verification requires generating the CBC blocks. That requires space of 2
  blocks per identity, and 1 CUDA core per identity for the same dataset. So as
many identities at once should be batched with as many proofs for those
identities verified concurrently for the same dataset.
* Validators will randomly sample the set of storage proofs to the set that
  they can handle, and only the creators of those chosen proofs will be
rewarded. The validator can run a benchmark whenever its hardware configuration
changes to determine what rate it can validate storage proofs.

## Validation and Replication Protocol

### Constants

1. NUM\_STORAGE\_ENTRIES: Number of entries in a segment of ledger data. The
unit of storage for a replicator.
2. NUM\_KEY\_ROTATION\_TICKS: Number of ticks to save a PoH value and cause a
key generation for the section of ledger just generated and the rotation of
another key in the set.
3. NUM\_STORAGE\_PROOFS: Number of storage proofs required for a storage proof
claim to be successfully rewarded.
4. RATIO\_OF\_FAKE\_PROOFS: Ratio of fake proofs to real proofs that a storage
mining proof claim has to contain to be valid for a reward.
5. NUM\_STORAGE\_SAMPLES: Number of samples required for a storage mining
proof.
6. NUM\_CHACHA\_ROUNDS: Number of encryption rounds performed to generate
encrypted state.

### Validator behavior

1. Validator joins the network and submits a storage validation capacity
transaction which tells the network how many proofs it can process in a given
period defined by NUM\_KEY\_ROTATION\_TICKS.
2. Every NUM\_KEY\_ROTATION\_TICKS the validator stores the PoH value at that
height.
3. Every NUM\_KEY\_ROTATION\_TICKS it also validates samples received from
replicators. It signs the PoH hash at that point and uses the following
algorithm with the signature as the input:
     - The low 5 bits of the first byte of the signature creates an index into
       another starting byte of the signature.
     - The validator then looks at the set of storage proofs where the byte of
       the proof's sha state vector starting from the low byte matches exactly
with the chosen byte(s) of the signature.
     - If the set of proofs is larger than the validator can handle, then it
       increases to matching 2 bytes in the signature.
     - Validator continues to increase the number of matching bytes until a
       workable set is found.
     - It then creates a mask of valid proofs and fake proofs and sends it to
       the leader. This is a storage proof confirmation transaction.
4. The storage proof confirmation transaction is integrated into the ledger.
5. After a lockout period of NUM\_SECONDS\_STORAGE\_LOCKOUT seconds, the
validator then submits a storage proof claim transaction which then causes the
distribution of the storage reward if no challenges were seen for the proof to
the validators and replicators party to the proofs.
6. Validator responds to RPC interfaces for what the last storage epoch PoH
value is and its entry\_height.

### Replicator behavior

1. Since a replicator is somewhat of a light client and not downloading all the
ledger data, they have to rely on other full nodes (validators) for
information. Any given validator may or may not be malicious and give incorrect
information, although there are not any obvious attack vectors that this could
accomplish besides having the replicator do extra wasted work.  For many of the
operations there are number of options depending on how paranoid a replicator
is:
    - (a) replicator can ask a validator
    - (b) replicator can ask multiple validators
    - (c) replicator can subscribe to the full transaction stream and generate
      the information itself
    - (d) replicator can subscribe to an abbreviated transaction stream to
      generate the information itself
2. A replicator obtains the PoH hash corresponding to the last key rotation
along with its entry\_height.
3. The replicator signs the PoH hash with its keypair. That signature is the
seed used to pick the segment to replicate and also the encryption key. The
replicator mods the signature with the entry\_height to get which segment to
replicate.
4. The replicator retrives the ledger by asking peer validators and
replicators. See 6.5.
5. The replicator then encrypts that segment with the key with chacha algorithm
in CBC mode with NUM\_CHACHA\_ROUNDS of encryption.
6. The replicator initializes a chacha rng with the signature from step 2 as
the seed.
7. The replicator generates NUM\_STORAGE\_SAMPLES samples in the range of the
entry size and samples the encrypted segment with sha256 for 32-bytes at each
offset value. Sampling the state should be faster than generating the encrypted
segment.
8. The replicator sends a PoRep proof transaction which contains its sha state
at the end of the sampling operation, its seed and the samples it used to the
current leader and it is put onto the ledger.
9. The replicator then generates another set of offsets which it submits a fake
proof with an incorrect sha state. It can be proven to be fake by providing the
seed for the hash result.
     - A fake proof should consist of a replicator hash of a signature of a PoH
       value. That way when the replicator reveals the fake proof, it can be
verified on chain.
10. The replicator monitors the ledger, if it sees a fake proof integrated, it
creates a challenge transaction and submits it to the current leader. The
transacation proves the validator incorrectly validated a fake storage proof.
The replicator is rewarded and the validator's staking balance is slashed or
frozen.

### Finding who has a given block of ledger

1. Validators monitor the transaction stream for storage mining proofs, and
keep a mapping of ledger segments by entry\_height to public keys. When it sees
a storage mining proof it updates this mapping and provides an RPC interface
which takes an entry\_height and hands back a list of public keys.  The client
then looks up in their cluster\_info table to see which network address that
corresponds to and sends a repair request to retrieve the necessary blocks of
ledger.
2. Validators would need to prune this list which it could do by periodically
looking at the oldest entries in its mappings and doing a network query to see
if the storage host is still serving the first entry.

## Sybil attacks

For any random seed, we force everyone to use a signature that is derived from
a PoH hash. Everyone must use the same count, so the same PoH hash is signed by
every participant. The signatures are then each cryptographically tied to the
keypair, which prevents a leader from grinding on the resulting value for more
than 1 identity.

Since there are many more client identities then encryption identities, we need
to split the reward for multiple clients, and prevent Sybil attacks from
generating many clients to acquire the same block of data. To remain BFT we
want to avoid a single human entity from storing all the replications of a
single chunk of the ledger.

Our solution to this is to force the clients to continue using the same
identity. If the first round is used to acquire the same block for many client
identities, the second round for the same client identities will force a
redistribution of the signatures, and therefore PoRep identities and blocks.
Thus to get a reward for replicators need to store the first block for free and
the network can reward long lived client identities more than new ones.

## Validator attacks

- If a validator approves fake proofs, replicator can easily out them by
  showing the initial state for the hash.
- If a validator marks real proofs as fake, no on-chain computation can be done
  to distinguish who is correct. Rewards would have to rely on the results from
multiple validators in a stake-weighted fashion to catch bad actors and
replicators from being locked out of the network.
- Validator stealing mining proof results for itself. The proofs are derived
  from a signature from a replicator, since the validator does not know the
private key used to generate the encryption key, it cannot be the generator of
the proof.

## Reward incentives

Fake proofs are easy to generate but difficult to verify. For this reason,
PoRep proof transactions generated by replicators may require a higher fee than
a normal transaction to represent the computational cost required by
validators.

Some percentage of fake proofs are also necessary to receive a reward from
storage mining.

## Notes

* We can reduce the costs of verification of PoRep by using PoH, and actually
  make it feasible to verify a large number of proofs for a global dataset.
* We can eliminate grinding by forcing everyone to sign the same PoH hash and
  use the signatures as the seed
* The game between validators and replicators is over random blocks and random
  encryption identities and random data samples. The goal of randomization is
to prevent colluding groups from having overlap on data or validation.
* Replicator clients fish for lazy validators by submitting fake proofs that
  they can prove are fake.
* Replication identities are just symmetric encryption keys, the number of them
  on the network is our storage replication target. Many more client identities
can exist than replicator identities, so unlimited number of clients can
provide proofs of the same replicator identity.
* To defend against Sybil client identities that try to store the same block we
  force the clients to store for multiple rounds before receiving a reward.
* Validators should also get rewarded for validating submitted storage proofs
  as incentive for storing the ledger. They can only validate proofs if they
are storing that slice of the ledger.
