# Ledger Replication

Replication behavior yet to be implemented.

### Storage epoch

The storage epoch should be the number of slots which results in around 100GB-1TB of
ledger to be generated for replicators to store. Replicators will start storing ledger
when a given fork has a high probability of not being rolled back.

### Validator behavior

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
5. After a lockout period of NUM\_SECONDS\_STORAGE\_LOCKOUT seconds, the
validator then submits a storage proof claim transaction which then causes the
distribution of the storage reward if no challenges were seen for the proof to
the validators and replicators party to the proofs.

### Replicator behavior

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

### Storage proof contract logic

Each replicator and validator will have their own storage account. The validator's
account would be separate from their gossip id similiar to their vote account.
These should be implemented as two programs one which handles the validator as the keysigner
and one for the replicator. In that way when the programs reference other accounts, they
can check the program id to ensure it is a validator or replicator account they are
referencing.

#### SubmitMiningProof
```rust,ignore
SubmitMiningProof {
    slot: u64,
    sha_state: Hash,
    signature: Signature,
};
keys = [replicator_keypair]
```
Replicators create these after mining their stored ledger data for a certain hash value.
The slot is the end slot of the segment of ledger they are storing, the sha\_state
the result of the replicator using the hash function to sample their encrypted ledger segment.
The signature is the signature that was created when they signed a PoH value for the
current storage epoch. The list of proofs from the current storage epoch should be saved
in the account state, and then transfered to a list of proofs for the previous epoch when
the epoch passes. In a given storage epoch a given replicator should only submit proofs
for one segment.

The program should have a list of slots which are valid storage mining slots.
This list should be maintained by keeping track of slots which are rooted slots in which a significant
portion of the network has voted on with a high lockout value, maybe 32-votes old. Every SLOTS\_PER\_SEGMENT
number of slots would be added to this set. The program should check that the slot is in this set. The set can
be maintained by receiving a AdvertiseStorageRecentBlockHash and checking with its bank/locktower state.

The program should do a signature verify check on the signature, public key from the transaction submitter and the message of
the previous storage epoch PoH value.

#### ProofValidation
```rust,ignore
ProofValidation {
   proof_mask: Vec<ProofStatus>,
}
keys = [validator_keypair, replicator_keypair(s) (unsigned)]
```
A validator will submit this transaction to indicate that a set of proofs for a given
segment are valid/not-valid or skipped where the validator did not look at it. The
keypairs for the replicators that it looked at should be referenced in the keys so the program
logic can go to those accounts and see that the proofs are generated in the previous epoch. The
sampling of the storage proofs should be verified ensuring that the correct proofs are skipped by
the validator according to the logic outlined in the validator behavior of sampling.

The included replicator keys will indicate the the storage samples which are being referenced; the
length of the proof\_mask should be verified against the set of storage proofs in the referenced
replicator account(s), and should match with the number of proofs submitted in the previous storage
epoch in the state of said replicator account.

#### ClaimStorageReward
```rust,ignore
ClaimStorageReward {
}
keys = [validator_keypair or replicator_keypair, validator/replicator_keypairs (unsigned)]
```
Replicators and validators will use this transaction to get paid tokens from a program state
where SubmitStorageProof, ProofValidation and ChallengeProofValidations are in a state where
proofs have been submitted and validated and there are no ChallengeProofValidations referencing
those proofs. For a validator, it should reference the replicator keypairs to which it has validated
proofs in the relevant epoch. And for a replicator it should reference validator keypairs for which it
has validated and wants to be rewarded.

#### ChallengeProofValidation
```rust,ignore
ChallengeProofValidation {
    proof_index: u64,
    hash_seed_value: Vec<u8>,
}
keys = [replicator_keypair, validator_keypair]
```

This transaction is for catching lazy validators who are not doing the work to validate proofs.
A replicator will submit this transaction when it sees a validator has approved a fake SubmitMiningProof
transaction. Since the replicator is a light client not looking at the full chain, it will have to ask
a validator or some set of validators for this information maybe via RPC call to obtain all ProofValidations for
a certain segment in the previous storage epoch. The program will look in the validator account
state see that a ProofValidation is submitted in the previous storage epoch and hash the hash\_seed\_value and
see that the hash matches the SubmitMiningProof transaction and that the validator marked it as valid. If so,
then it will save the challenge to the list of challenges that it has in its state.

#### AdvertiseStorageRecentBlockhash
```rust,ignore
AdvertiseStorageRecentBlockhash {
    hash: Hash,
    slot: u64,
}
```

Validators and replicators will submit this to indicate that a new storage epoch has passed and that the
storage proofs which are current proofs should now be for the previous epoch. Other transactions should
check to see that the epoch that they are referencing is accurate according to current chain state.
