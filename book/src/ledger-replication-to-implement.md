# Ledger Replication

Replication behavior yet to be implemented.

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

