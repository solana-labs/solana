# Ledger Replication

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

## Optimization with PoH

Our improvement on this approach is to randomly sample the encrypted segments
faster than it takes to encrypt, and record the hash of those samples into the
PoH ledger. Thus the segments stay in the exact same order for every PoRep and
verification can stream the data and verify all the proofs in a single batch.
This way we can verify multiple proofs concurrently, each one on its own CUDA
core. The total space required for verification is `1_ledger_segment +
2_cbc_blocks * number_of_identities` with core count equal to
`number_of_identities`. We use a 64-byte chacha CBC block size.

## Network

Validators for PoRep are the same validators that are verifying transactions.
They have some stake that they have put up as collateral that ensures that
their work is honest. If you can prove that a validator verified a fake PoRep,
then the validator will not receive a reward for that storage epoch.

Replicators are specialized *light clients*. They download a part of the ledger
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

1. SLOTS\_PER\_SEGMENT: Number of slots in a segment of ledger data. The
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
3. Validator generates a storage proof confirmation transaction.
4. The storage proof confirmation transaction is integrated into the ledger.
6. Validator responds to RPC interfaces for what the last storage epoch PoH
value is and its slot.

### Replicator behavior

1. Since a replicator is somewhat of a light client and not downloading all the
ledger data, they have to rely on other full nodes (validators) for
information. Any given validator may or may not be malicious and give incorrect
information, although there are not any obvious attack vectors that this could
accomplish besides having the replicator do extra wasted work.  For many of the
operations there are a number of options depending on how paranoid a replicator
is:
    - (a) replicator can ask a validator
    - (b) replicator can ask multiple validators
    - (c) replicator can subscribe to the full transaction stream and generate
      the information itself
    - (d) replicator can subscribe to an abbreviated transaction stream to
      generate the information itself
2. A replicator obtains the PoH hash corresponding to the last key rotation
along with its slot.
3. The replicator signs the PoH hash with its keypair. That signature is the
seed used to pick the segment to replicate and also the encryption key. The
replicator mods the signature with the slot to get which segment to
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


### Finding who has a given block of ledger

1. Validators monitor the transaction stream for storage mining proofs, and
keep a mapping of ledger segments by slot to public keys. When it sees
a storage mining proof it updates this mapping and provides an RPC interface
which takes a slot and hands back a list of public keys.  The client
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
* To defend against Sybil client identities that try to store the same block we
  force the clients to store for multiple rounds before receiving a reward.
* Validators should also get rewarded for validating submitted storage proofs
  as incentive for storing the ledger. They can only validate proofs if they
are storing that slice of the ledger.
