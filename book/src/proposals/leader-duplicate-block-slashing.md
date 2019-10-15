# Leader Duplicate Block Slashing

This design describes how the cluster slashes leaders that produce
duplicate blocks.

Leaders that produce multiple blocks for the same slot increase the
number of potential forks that the cluster has to resolve.

## Shred Format

Shreds are produced by leaders during their scheduled slot.  Each
shred is signed by the leader and is transmitted to the cluster via
Turbine. A shred contains the following

* Signature
* header: slot, shred index
* msg

The signature is of the merkle tree of the shred data.

```
              merke root
    /                        \
(slot index, shred index)        hash(msg)
```

## Proof of a Duplicate Shred

Any two different signatures that show a merkle path to the same
`(slot index, shred index)` for the same leader are proof that the
leader produced two conflicting blocks.

## Avoiding Accidental Slashing

Leaders could be killed and restarted in the middle of their block,
and loose any information that they had started producing the block.
Leaders should store the last block that the leader had started
producing in persistent storage before sending the first shred.

If the persistent storage is corrupted, leaders need to wait long
enough to observe a round of consensus in which the leader has
participated before starting producing blocks.  Waiting for
confirmation that their own signatures are included in the next
block ensures that the blocks the leader is observing are new and
not replayed.
