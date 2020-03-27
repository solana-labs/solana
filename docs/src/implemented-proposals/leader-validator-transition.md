# Leader-to-Validator Transition

A validator typically spends its time validating blocks. If, however, a staker delegates its stake to a validator, it will occasionally be selected as a _slot leader_. As a slot leader, the validator is responsible for producing blocks during an assigned _slot_. A slot has a duration of some number of preconfigured _ticks_. The duration of those ticks are estimated with a _PoH Recorder_ described later in this document.

## BankFork

BankFork tracks changes to the bank state over a specific slot. Once the final tick has been registered the state is frozen. Any attempts to write to are rejected.

## Validator

A validator operates on many different concurrent forks of the bank state until it generates a PoH hash with a height within its leader slot.

## Slot Leader

A slot leader builds blocks on top of only one fork, the one it last voted on.

## PoH Recorder

Slot leaders and validators use a PoH Recorder for both estimating slot height and for recording transactions.

### PoH Recorder when Validating

The PoH Recorder acts as a simple VDF when validating. It tells the validator when it needs to switch to the slot leader role. Every time the validator votes on a fork, it should use the fork's latest [blockhash](../terminology.md#blockhash) to re-seed the VDF. Re-seeding solves two problems. First, it synchronizes its VDF to the leader's, allowing it to more accurately determine when its leader slot begins. Second, if the previous leader goes down, all wallclock time is accounted for in the next leader's PoH stream. For example, if one block is missing when the leader starts, the block it produces should have a PoH duration of two blocks. The longer duration ensures the following leader isn't attempting to snip all the transactions from the previous leader's slot.

### PoH Recorder when Leading

A slot leader use the PoH Recorder to record transactions, locking their positions in time. The PoH hash must be derived from a previous leader's last block. If it isn't, its block will fail PoH verification and be rejected by the cluster.

The PoH Recorder also serves to inform the slot leader when its slot is over. The leader needs to take care not to modify its bank if recording the transaction would generate a PoH height outside its designated slot. The leader, therefore, should not commit account changes until after it generates the entry's PoH hash. When the PoH height falls outside its slot any transactions in its pipeline may be dropped or forwarded to the next leader. Forwarding is preferred, as it would minimize network congestion, allowing the cluster to advertise higher TPS capacity.

## Validator Loop

The PoH Recorder manages the transition between modes. Once a ledger is replayed, the validator can run until the recorder indicates it should be the slot leader. As a slot leader, the node can then execute and record transactions.

The loop is synchronized to PoH and does a synchronous start and stop of the slot leader functionality. After stopping, the validator's TVU should find itself in the same state as if a different leader had sent it the same block. The following is pseudocode for the loop:

1. Query the LeaderScheduler for the next assigned slot.
2. Run the TVU over all the forks. 1. TVU will send votes to what it believes is the "best" fork. 2. After each vote, restart the PoH Recorder to run until the next assigned

   slot.

3. When time to be a slot leader, start the TPU. Point it to the last fork the

   TVU voted on.

4. Produce entries until the end of the slot. 1. For the duration of the slot, the TVU must not vote on other forks. 2. After the slot ends, the TPU freezes its BankFork. After freezing,

   the TVU may resume voting.

5. Goto 1.

