# Rollback

Rollback is necessary when leader misbehavior has the potential to be discovered after validators have already replayed a slot to completion.

# Scenarios where Rollback is Necessary:

1) Leader emits different a different set of blobs for the same slot. 

Detection: Before every insert into blocktree, check if an existing blob for the slot and index already exists. If so, verify that the existing blob and the new blob have the same data signature. If not, execute the below "Rollback Protocol".

Proof of Misbehavior: Two blobs with the same slot and index, but differing data signature. When a validator detects such a situation

Proof Propagation: A validator that detects this should present the proof over gossip to all other validators. Proof of misbehavior in slot "N" should be kept around in gossip until slot "N + M" for some configurable constant "M".

# Rollback Protocol

1) On seeing proof of leader misbehavior, validators mark that slot in the DeadSlots column family in Blocktree. This will prevent replay_stage from observing or replaying any descendants of this fork.

2) If the corrupted slot is an ancestor of the last vote, reset PoH to the ancestor of the corrupted slot. In case there's a large gap between the last ancestor of the corrupted slot and the corrupted slot itself, we should cache the intermediate PoH ticks in Blocktree so that we do not have to reset the PoH as far. For instance if the chain of slots is:

1 -> 2 -> 100 -> 101

If validators detect slot 100 is corrupted, resetting to slot 2 is very expensive because we have to generate all the virtual ticks between slots 2 and 100 again. Instead, if the validators cached those ticks, we do not need to reset PoH as far.


3) Remove any descendants of the corrupted slot from BankForks.

