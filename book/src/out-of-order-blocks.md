# Out Of Order Blocks

The [deterministic leader schedule](../tree/master/book/src/leader-rotation.md)
creates a fixed schedule for block production.  The main problems with this
approach are as follows:

* The leader schedule has to balance both performance and liveness requirements.
The epoch needs to be long enough to guarantee liveness.

* The cluster has to wait for unavailable nodes.

* The cluster's liveness depends on the availability of scheduled nodes.

* Attackers know in advance all of the leaders that propose a block.

The general problems of DPOS — that a cluster requires an interactive protocol
for account registration, delegation, and voting that a set of leaders can
censor — is not addressed in this design.

## Random Leaders

Leaders can generate a *golden ticket* to propose a block.

* *N* number of bits of a leader's vote signature that must match a PoH hash.
This hash can be in between ticks.

The leader must find the golden ticket in the empty PoH entries leading up to
the proposed block.  Once the golden ticket is found, the leader can propose the
next slot.  The leader signs the BlockHash of the block that the entries are
generated from with a Vote.  Since each validator is resetting its PoH every
time it votes, each validator can search for a golden ticket from that starting
point.

The same proposed [fork selection]/fork-selection.md) rules apply.  Once a block
is voted on, the validator would switch its PoH to reset on this new block, and
either a new golden ticket block is proposed, or the next scheduled leader
proposes a block.

To make it easy to verify, the vote with the golden ticket must be present in
the proposed block as the first transaction.

### Grinding

Because the golden ticket is biasable, the last leader has the most opportunity
to influence the BlockHash to generate the golden ticket for itself.  This
BlockHash could be a grinding attack with N cores.  Grinding should be a limited
attack vector, since the difficulty is determined by the size of the delegates
to the leaders’ VoteState account.

To reduce the ability to grind the value, the golden ticket must appear *X*
ticks after the block, where *X* is half of the number of ticks per block.

### Difficulty

Difficulty is the number of matching bits that the golden ticket needs to match
the signature.

The golden ticket connects via PoH entries to a known block with an immutable
account state.  The stake size can be used to set the difficulty.  Hashing with
1 account has been delegated 100 tokens should generate as many tickets as 2
accounts with 50 delegated tokens.

Given the total delegated stake size for the epoch, and number of hashes per
Tick, the difficulty can be computed with the leader scheduler.

### Generating VDFs for Multiple Forks

The number of potential forks the attacker can choose from is limited by
the fork selection algorithm.  The generated golden ticket needs to be past the
lockout distance for the rest of the cluster and also be the heaviest fork
based on lockout.  Let’s say the difficulty was set to the cluster generating a
golden ticket block every 3 slots.

For example:

Starting with ‘n - 3’ forks, the proposed block would only be valid if no one in
the cluster voted.  If they did, the golden ticket would need to be in the ‘n +
7’ slot and propose the n + 8 block.

### Transaction Ingress Point

Because there is no deterministic ingress point, there is no place for
validators to forward transactions.  This design will end up requiring a shared
*mempool* of transactions between all the nodes, just like traditional PoW
systems.

The validators already transmit their votes through gossip, so the golden ticket
block can pick up the validator votes and any additional transactions that have
not been encoded into the ledger.  These transactions could come from rejected
forks.

## Only Random Leaders

If the random source is considered secure, it is possible to generate a leader
schedule completely randomly.

1. Leader 2 finds the golden ticket in slot 1
2. Leader 2 proposes block for slot 2
3. Block contains up to two PoH paths:
    * The golden ticket PoH.
    * The PoH path to a previous block.

The golden ticket PoH path must be at most 1 slot longer that the PoH path to
the block, and this should only occur if the block is attached to the previous
block.  The reason why the two paths could be different is because Leader 1 may
complete the block for slot 1 and Leader 2 may decide to attach block 2 to block
1.

### Grinding

If it is possible to build hardware that is much faster than the expected time
to find the golden ticket, then an attacker could grind the Block Hash value to
ensure that they are the leader 50% of the time.  Since the network is still
relying on staked lamports as the Sybil resistant mechanism, we could increase
the difficulty for consequtive stakes.
