---
title: Optimistic Transaction Propagation Signal
---

## Current Retransmit behavior

The retransmission tree currently considers:
1. epoch staked nodes
2. tvu peers (filtered by contact info and shred version)
3. current validator

concatenating (1), (2), and (3)
deduplicating this list of entries by pubkey favoring entries with contact info
filtering this list by entries with contact info

This list is then is randomly shuffled by stake weight.

Shreds are then retransmitted to up to FANOUT neighbors and up to FANOUT
children.

## Deterministic retransmission tree

`weighted_shuffle` will use a deterministic seed when
`enable_deterministic_seed` has been enabled based on the triple (shred slot,
shred index, leader pubkey):

```
if enable_deterministic_seed(self.slot(), root_bank) {
    hashv(&[
        &self.slot().to_le_bytes(),
        &self.index().to_le_bytes(),
        &leader_pubkey.to_bytes(),
    ])
```

First, only epoch staked nodes will be considered regardless of presence of
contact info (and possibly including the validator node itself).

A deterministic ordering of the epoch staked nodes will be created based on the
derministic shred seed using weighted_shuffle.

Let `neighbor_set` be selected from up to FANOUT neighbors of the current node.
Let `child_set` be selected from up to FANOUT children of the current node.

Filter `neighbor_set` by contact info.
Filter `child_set` by contact info.

Let `epoch_set` be the union of `neighbor_set` and `child_set`.

Let `remaining_set` be all other nodes with contact info not contained in
`epoch_set`.

If `epoch_set.len < 2*FANOUT` then we may randomly select up to
`2*FANOUT - epoch_set.len` nodes to to retransmit to from `remaining_set`.

## Receiving retransmitted shred

If the current validator node is not in the set of epoch staked nodes for the
shred epoch then no early retransmission information can be obtained.

Compute the deterministic shred seed.

Run the deterministic epoch_stakes shuffle.

Find position of self in the neighbor or child sets.

Calculate the sum of the stakes of all nodes in the current and prior
distribution levels.

### Stake summation considerations:

- Stake sum could include stakes of nodes which had been skipped in prior
distribution levels because of lack of contact info.
- Current node was part of original epoch staked shuffle from retransmitter
but was filtered out because of missing contact info. Current node subsequently
receives retransmisison of shred and assumes that the retransmit was a result
of the deterministic tree calculation and not from subsequent random selection.
This should be benign because the current node will underestimate prior stake
weight in the retransmission tree.

### General considerations:

attack by leader (level 0):
- transmits shred for distribution through the tree as normal
- additionally transmits shred (or fake shred) directly to node(s) at level >=2
leading the node(s) to believe a greater percentage of the tree retransmission
tree had been processed

attack by node at level n:
- retransmits shred to node(s) at level >=n+2 leading the node(s) to believe a
greater percentage of the tree retransmission tree had been processed

### Questions

- Should receiving nodes attempt to verify that the origin of the shred was
retransmitted from the expected node? If so, consideration of spoofing?
- How is this information consumed?

## Notes

Practically, signals should fall into the following buckets:
1. current leader (can signal layer 1 when broadcast is sent)
2. layer 1
1.1. can signal layer 1 when shred is received
1.2. can signal layer 1 + subset of layer 2 when retransmit is sent
3. layer 2
3.1. can signal layer 2 when shred is received
3.2. can signal layer 2 + subset of layer 3 when retrnasmit is sent
4. current node not a member of epoch staked nodes, no signal can be sent
