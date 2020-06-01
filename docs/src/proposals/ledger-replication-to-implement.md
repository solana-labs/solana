# Ledger Replication

Note: this ledger replication solution was partially implemented, but not
completed. The partial implementation was removed by
https://github.com/solana-labs/solana/pull/9992 in order to prevent the security
risk of unused code. The first part of this design document reflects the
once-implemented parts of ledger replication. The
[second part of this document](#ledger-replication-not-implemented) describes the
parts of the solution never implemented.

## Proof of Replication

At full capacity on a 1gbps network solana will generate 4 petabytes of data per year. To prevent the network from centralizing around validators that have to store the full data set this protocol proposes a way for mining nodes to provide storage capacity for pieces of the data.

The basic idea to Proof of Replication is encrypting a dataset with a public symmetric key using CBC encryption, then hash the encrypted dataset. The main problem with the naive approach is that a dishonest storage node can stream the encryption and delete the data as it's hashed. The simple solution is to periodically regenerate the hash based on a signed PoH value. This ensures that all the data is present during the generation of the proof and it also requires validators to have the entirety of the encrypted data present for verification of every proof of every identity. So the space required to validate is `number_of_proofs * data_size`

## Optimization with PoH

Our improvement on this approach is to randomly sample the encrypted segments faster than it takes to encrypt, and record the hash of those samples into the PoH ledger. Thus the segments stay in the exact same order for every PoRep and verification can stream the data and verify all the proofs in a single batch. This way we can verify multiple proofs concurrently, each one on its own CUDA core. The total space required for verification is `1_ledger_segment + 2_cbc_blocks * number_of_identities` with core count equal to `number_of_identities`. We use a 64-byte chacha CBC block size.

## Network

Validators for PoRep are the same validators that are verifying transactions. If an archiver can prove that a validator verified a fake PoRep, then the validator will not receive a reward for that storage epoch.

Archivers are specialized _light clients_. They download a part of the ledger \(a.k.a Segment\) and store it, and provide PoReps of storing the ledger. For each verified PoRep archivers earn a reward of sol from the mining pool.

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

   unit of storage for an archiver.

2. NUM\_KEY\_ROTATION\_SEGMENTS: Number of segments after which archivers

   regenerate their encryption keys and select a new dataset to store.

3. NUM\_STORAGE\_PROOFS: Number of storage proofs required for a storage proof

   claim to be successfully rewarded.

4. RATIO\_OF\_FAKE\_PROOFS: Ratio of fake proofs to real proofs that a storage

   mining proof claim has to contain to be valid for a reward.

5. NUM\_STORAGE\_SAMPLES: Number of samples required for a storage mining

   proof.

6. NUM\_CHACHA\_ROUNDS: Number of encryption rounds performed to generate

   encrypted state.

7. NUM\_SLOTS\_PER\_TURN: Number of slots that define a single storage epoch or

   a "turn" of the PoRep game.

### Validator behavior

1. Validators join the network and begin looking for archiver accounts at each

   storage epoch/turn boundary.

2. Every turn, Validators sign the PoH value at the boundary and use that signature

   to randomly pick proofs to verify from each storage account found in the turn boundary.

   This signed value is also submitted to the validator's storage account and will be used by

   archivers at a later stage to cross-verify.

3. Every `NUM_SLOTS_PER_TURN` slots the validator advertises the PoH value. This is value

   is also served to Archivers via RPC interfaces.

4. For a given turn N, all validations get locked out until turn N+3 \(a gap of 2 turn/epoch\).

   At which point all validations during that turn are available for reward collection.

5. Any incorrect validations will be marked during the turn in between.

### Archiver behavior

1. Since an archiver is somewhat of a light client and not downloading all the

   ledger data, they have to rely on other validators and archivers for information.

   Any given validator may or may not be malicious and give incorrect information, although

   there are not any obvious attack vectors that this could accomplish besides having the

   archiver do extra wasted work. For many of the operations there are a number of options

   depending on how paranoid an archiver is:

   * \(a\) archiver can ask a validator
   * \(b\) archiver can ask multiple validators
   * \(c\) archiver can ask other archivers
   * \(d\) archiver can subscribe to the full transaction stream and generate

     the information itself \(assuming the slot is recent enough\)

   * \(e\) archiver can subscribe to an abbreviated transaction stream to

     generate the information itself \(assuming the slot is recent enough\)

2. An archiver obtains the PoH hash corresponding to the last turn with its slot.
3. The archiver signs the PoH hash with its keypair. That signature is the

   seed used to pick the segment to replicate and also the encryption key. The

   archiver mods the signature with the slot to get which segment to

   replicate.

4. The archiver retrives the ledger by asking peer validators and

   archivers. See 6.5.

5. The archiver then encrypts that segment with the key with chacha algorithm

   in CBC mode with `NUM_CHACHA_ROUNDS` of encryption.

6. The archiver initializes a chacha rng with the a signed recent PoH value as

   the seed.

7. The archiver generates `NUM_STORAGE_SAMPLES` samples in the range of the

   entry size and samples the encrypted segment with sha256 for 32-bytes at each

   offset value. Sampling the state should be faster than generating the encrypted

   segment.

8. The archiver sends a PoRep proof transaction which contains its sha state

   at the end of the sampling operation, its seed and the samples it used to the

   current leader and it is put onto the ledger.

9. During a given turn the archiver should submit many proofs for the same segment

   and based on the `RATIO_OF_FAKE_PROOFS` some of those proofs must be fake.

10. As the PoRep game enters the next turn, the archiver must submit a

    transaction with the mask of which proofs were fake during the last turn. This

    transaction will define the rewards for both archivers and validators.

11. Finally for a turn N, as the PoRep game enters turn N + 3, archiver's proofs for

    turn N will be counted towards their rewards.

### The PoRep Game

The Proof of Replication game has 4 primary stages. For each "turn" multiple PoRep games can be in progress but each in a different stage.

The 4 stages of the PoRep Game are as follows:

1. Proof submission stage
   * Archivers: submit as many proofs as possible during this stage
   * Validators: No-op
2. Proof verification stage
   * Archivers: No-op
   * Validators: Select archivers and verify their proofs from the previous turn
3. Proof challenge stage
   * Archivers: Submit the proof mask with justifications \(for fake proofs submitted 2 turns ago\)
   * Validators: No-op
4. Reward collection stage
   * Archivers: Collect rewards for 3 turns ago
   * Validators:  Collect rewards for 3 turns ago

For each turn of the PoRep game, both Validators and Archivers evaluate each stage. The stages are run as separate transactions on the storage program.

### Finding who has a given block of ledger

1. Validators monitor the turns in the PoRep game and look at the rooted bank

   at turn boundaries for any proofs.

2. Validators maintain a map of ledger segments and corresponding archiver public keys.

   The map is updated when a Validator processes an archiver's proofs for a segment.

   The validator provides an RPC interface to access the this map. Using this API, clients

   can map a segment to an archiver's network address \(correlating it via cluster\_info table\).

   The clients can then send repair requests to the archiver to retrieve segments.

3. Validators would need to invalidate this list every N turns.

## Sybil attacks

For any random seed, we force everyone to use a signature that is derived from a PoH hash at the turn boundary. Everyone uses the same count, so the same PoH hash is signed by every participant. The signatures are then each cryptographically tied to the keypair, which prevents a leader from grinding on the resulting value for more than 1 identity.

Since there are many more client identities then encryption identities, we need to split the reward for multiple clients, and prevent Sybil attacks from generating many clients to acquire the same block of data. To remain BFT we want to avoid a single human entity from storing all the replications of a single chunk of the ledger.

Our solution to this is to force the clients to continue using the same identity. If the first round is used to acquire the same block for many client identities, the second round for the same client identities will force a redistribution of the signatures, and therefore PoRep identities and blocks. Thus to get a reward for archivers need to store the first block for free and the network can reward long lived client identities more than new ones.

## Validator attacks

* If a validator approves fake proofs, archiver can easily out them by

  showing the initial state for the hash.

* If a validator marks real proofs as fake, no on-chain computation can be done

  to distinguish who is correct. Rewards would have to rely on the results from

  multiple validators to catch bad actors and archivers from being denied rewards.

* Validator stealing mining proof results for itself. The proofs are derived

  from a signature from an archiver, since the validator does not know the

  private key used to generate the encryption key, it cannot be the generator of

  the proof.

## Reward incentives

Fake proofs are easy to generate but difficult to verify. For this reason, PoRep proof transactions generated by archivers may require a higher fee than a normal transaction to represent the computational cost required by validators.

Some percentage of fake proofs are also necessary to receive a reward from storage mining.

## Notes

* We can reduce the costs of verification of PoRep by using PoH, and actually

  make it feasible to verify a large number of proofs for a global dataset.

* We can eliminate grinding by forcing everyone to sign the same PoH hash and

  use the signatures as the seed

* The game between validators and archivers is over random blocks and random

  encryption identities and random data samples. The goal of randomization is

  to prevent colluding groups from having overlap on data or validation.

* Archiver clients fish for lazy validators by submitting fake proofs that

  they can prove are fake.

* To defend against Sybil client identities that try to store the same block we

  force the clients to store for multiple rounds before receiving a reward.

* Validators should also get rewarded for validating submitted storage proofs

  as incentive for storing the ledger. They can only validate proofs if they

  are storing that slice of the ledger.

# Ledger Replication Not Implemented

Replication behavior yet to be implemented.

## Storage epoch

The storage epoch should be the number of slots which results in around 100GB-1TB of ledger to be generated for archivers to store. Archivers will start storing ledger when a given fork has a high probability of not being rolled back.

## Validator behavior

1. Every NUM\_KEY\_ROTATION\_TICKS it also validates samples received from

   archivers. It signs the PoH hash at that point and uses the following

   algorithm with the signature as the input:

   * The low 5 bits of the first byte of the signature creates an index into

     another starting byte of the signature.

   * The validator then looks at the set of storage proofs where the byte of

     the proof's sha state vector starting from the low byte matches exactly

     with the chosen byte\(s\) of the signature.

   * If the set of proofs is larger than the validator can handle, then it

     increases to matching 2 bytes in the signature.

   * Validator continues to increase the number of matching bytes until a

     workable set is found.

   * It then creates a mask of valid proofs and fake proofs and sends it to

     the leader. This is a storage proof confirmation transaction.

2. After a lockout period of NUM\_SECONDS\_STORAGE\_LOCKOUT seconds, the

   validator then submits a storage proof claim transaction which then causes the

   distribution of the storage reward if no challenges were seen for the proof to

   the validators and archivers party to the proofs.

## Archiver behavior

1. The archiver then generates another set of offsets which it submits a fake

   proof with an incorrect sha state. It can be proven to be fake by providing the

   seed for the hash result.

   * A fake proof should consist of an archiver hash of a signature of a PoH

     value. That way when the archiver reveals the fake proof, it can be

     verified on chain.

2. The archiver monitors the ledger, if it sees a fake proof integrated, it

   creates a challenge transaction and submits it to the current leader. The

   transacation proves the validator incorrectly validated a fake storage proof.

   The archiver is rewarded and the validator's staking balance is slashed or

   frozen.

## Storage proof contract logic

Each archiver and validator will have their own storage account. The validator's account would be separate from their gossip id similiar to their vote account. These should be implemented as two programs one which handles the validator as the keysigner and one for the archiver. In that way when the programs reference other accounts, they can check the program id to ensure it is a validator or archiver account they are referencing.

### SubmitMiningProof

```text
SubmitMiningProof {
    slot: u64,
    sha_state: Hash,
    signature: Signature,
};
keys = [archiver_keypair]
```

Archivers create these after mining their stored ledger data for a certain hash value. The slot is the end slot of the segment of ledger they are storing, the sha\_state the result of the archiver using the hash function to sample their encrypted ledger segment. The signature is the signature that was created when they signed a PoH value for the current storage epoch. The list of proofs from the current storage epoch should be saved in the account state, and then transfered to a list of proofs for the previous epoch when the epoch passes. In a given storage epoch a given archiver should only submit proofs for one segment.

The program should have a list of slots which are valid storage mining slots. This list should be maintained by keeping track of slots which are rooted slots in which a significant portion of the network has voted on with a high lockout value, maybe 32-votes old. Every SLOTS\_PER\_SEGMENT number of slots would be added to this set. The program should check that the slot is in this set. The set can be maintained by receiving a AdvertiseStorageRecentBlockHash and checking with its bank/Tower BFT state.

The program should do a signature verify check on the signature, public key from the transaction submitter and the message of the previous storage epoch PoH value.

### ProofValidation

```text
ProofValidation {
   proof_mask: Vec<ProofStatus>,
}
keys = [validator_keypair, archiver_keypair(s) (unsigned)]
```

A validator will submit this transaction to indicate that a set of proofs for a given segment are valid/not-valid or skipped where the validator did not look at it. The keypairs for the archivers that it looked at should be referenced in the keys so the program logic can go to those accounts and see that the proofs are generated in the previous epoch. The sampling of the storage proofs should be verified ensuring that the correct proofs are skipped by the validator according to the logic outlined in the validator behavior of sampling.

The included archiver keys will indicate the the storage samples which are being referenced; the length of the proof\_mask should be verified against the set of storage proofs in the referenced archiver account\(s\), and should match with the number of proofs submitted in the previous storage epoch in the state of said archiver account.

### ClaimStorageReward

```text
ClaimStorageReward {
}
keys = [validator_keypair or archiver_keypair, validator/archiver_keypairs (unsigned)]
```

Archivers and validators will use this transaction to get paid tokens from a program state where SubmitStorageProof, ProofValidation and ChallengeProofValidations are in a state where proofs have been submitted and validated and there are no ChallengeProofValidations referencing those proofs. For a validator, it should reference the archiver keypairs to which it has validated proofs in the relevant epoch. And for an archiver it should reference validator keypairs for which it has validated and wants to be rewarded.

### ChallengeProofValidation

```text
ChallengeProofValidation {
    proof_index: u64,
    hash_seed_value: Vec<u8>,
}
keys = [archiver_keypair, validator_keypair]
```

This transaction is for catching lazy validators who are not doing the work to validate proofs. An archiver will submit this transaction when it sees a validator has approved a fake SubmitMiningProof transaction. Since the archiver is a light client not looking at the full chain, it will have to ask a validator or some set of validators for this information maybe via RPC call to obtain all ProofValidations for a certain segment in the previous storage epoch. The program will look in the validator account state see that a ProofValidation is submitted in the previous storage epoch and hash the hash\_seed\_value and see that the hash matches the SubmitMiningProof transaction and that the validator marked it as valid. If so, then it will save the challenge to the list of challenges that it has in its state.

### AdvertiseStorageRecentBlockhash

```text
AdvertiseStorageRecentBlockhash {
    hash: Hash,
    slot: u64,
}
```

Validators and archivers will submit this to indicate that a new storage epoch has passed and that the storage proofs which are current proofs should now be for the previous epoch. Other transactions should check to see that the epoch that they are referencing is accurate according to current chain state.
