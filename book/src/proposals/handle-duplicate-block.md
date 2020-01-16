# Leader Duplicate Block Slashing

This design describes how the cluster slashes leaders that produce duplicate
blocks.

Leaders that produce multiple blocks for the same slot increase the number of
potential forks that the cluster has to resolve.

## Procedure
1. Blockstore changes:
    * a. Augment `DeadSlots` keyspace in Blockstore to be an Option<Blockhash>.
    This is called the `approved_blockhash`.
    * b. Augment the `Roots` column family to include the blockhash of the
    rooted banks

2. Once the CheckDuplicate thread detects a duplicate slot, it:
    * a. Stores a proof of the two duplicate shreds for that slot
    * b. Sends a signal to ReplayStage for that slot.

3. Once ReplayStage receives a signal about a duplicate slot, it checks if
the current version with blockhash `B` has less than 2^THRESHOLD lockout. 
    * a. If so, then remove that slot and all its children from the progress
    map. Mark the slot as `dead` in `DeadSlots`, with `approved_blockhash`
    set to `None`.
    * b. Otherwise, check `!approved_blockhash.is_none()`.
        * i. If true, set the `approved_blockhash` for that slot to `B` in 
    `DeadSlots`. This is why `1b` is important, because banks for slots 
    earlier than the root will have been purged by BankForks and thus 
    the blockhash will not be available in memory.
        * ii. If false, this hash must have been set by step `6b`, in which
    case, check that the existing hash matches `B`. If not true, throw an
    error because that meanss >33% of the cluster is malicious.
    * c. When fetching new slots in ReplayStage, a slot is not replayed if it is
    dead, unless an `approved_blockhash` is set.

    Note: It's posssible the slot is already dead when ReplayStage receives
    the duplicate signal. In this case the `approved_blockhash` in `DeadSlots`
    will also be set as `None`. This case is be handled by step `5` below if another
    playble version of this slot gains approvals (The "approved blockhash" 
    will be set).

4. WindowService will now stop accepting shreds for dead slots or shreds with
parents chaining to dead slots, unless the shred is also: 
    * a. A repair request
    * b. For the `approved_blockhash` (TODO: Need a way to confirm this, as 
    repair requests are currently too small to contain another hash.
    Probably will need a merkle)
    
    Thus, a duplicate slot marked "dead" by ReplayStage will not receive further
    shreds unless an "approved blockhassh" is set.

5. Repair thread iterates over the set of slots in Blocktree that are:
    * a. Greater than the root
    * b. The slot exists in `DeadSlots` in Blockstore but the 
       `approved_blockhash` in `DeadSlots` is `None` (implies ReplayStage 
       has either gotten a signal from the CheckDuplicate thread, or
       has seen a bad version of this slot).
    * c. The slot exists in `DuplicateSlots` in Blockstore

    For each of these slots that passes the above criteria, the repair thread
    queries the cluster's validators about their `approved_blockhash`. 

    If the repair thread sees >33% of validators with the same `approved_blockhash`
    `B`, then that means the following condition must be true:

    `There are > 66% of people who have voted on blockhash B`

    This is because there are <= 33% malicious validators on the network, and
    `> 33%` responded with the same `approved_blockhash`, so there is at least
    one correct validator that saw the above condition hold. This is then a
    safe version of the slot to repair because no correct validator can be locked
    out more than `2^THRESHOLD` on another version of this slot, and thus all
    correct validators will either repair this version off the slot, or skip 
    this slot.

    The repair thread then sends an `Approved Blockhash` signal to ReplayStage

6. Upon receiving the `Approved Blockhash` signal, ReplayStage checks if 
an `approved_blockhash` is equal to `None`. If not:
    * a. Clear the slot-related columns in Blockstore. This is safe because
    there are no simultaneous writes to these columns from WindowService as 
    guaranteed by step `4`. 
    * b. Set the `approved_blockhash` in DeadSlots, allowing ReplayStage to once 
    again replay this slot. (see step `3c`).

  If the hash does exist, run step `3.b.ii` above.