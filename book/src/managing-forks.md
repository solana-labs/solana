# Managing Forks in the Ledger

The ledger is permitted to fork at slot boundaries. The resulting data
structure forms a tree called a *blocktree*. When the fullnode interprets the
blocktree, it must maintain state for each fork in the chain. We call each
instance an *active fork*.  It is the responsibility of a fullnode to weigh
those forks, such that it may eventually select a fork.

A fullnode selects a fork by submiting a vote to a slot leader on that fork.
The vote commits the fullnode for a duration of time called a *lockout period*.
The fullnode is not permitted to vote on a different fork until that lockout
period expires. Each subsequent vote on the same fork doubles the length of the
lockout period. After some cluster-configured number of votes (currently 32),
the length of the lockout period reaches what's called *max lockout*. Until the
max lockout is reached, the fullnode has the option to wait until the lockout
period is over and then vote on another fork. When it votes on another fork, it
performs a operation called *rollback*, whereby the state rolls back in time to
a shared checkpoint and then jumps forward to the tip of the fork that it just
voted on. The maximum distance that a fork may roll back is called the
*rollback depth*. Rollback depth is the number of votes required to achieve
max lockout. Whenever a fullnode votes, any checkpoints beyond the rollback
depth become unreachable. That is, there is no scenario in which the fullnode
will need to roll back beyond rollback depth. It therefore may safely *prune*
unreachable forks and *squash* all checkpoints beyond rollback depth into the
root checkpoint.

## Active Forks

An active fork is as a sequence of checkpoints that has a length at least one
longer than the rollback depth. The shortest fork will have a length exactly
one longer than the rollback depth.  For example:

<img alt="Forks" src=".gitbook/assets/forks.svg" class="center"/>

The following sequences are *active forks*:

* {4, 2, 1}
* {5, 2, 1}
* {6, 3, 1}
* {7, 3, 1}

## Pruning and Squashing

A fullnode may vote on any checkpoint in the tree.  In the diagram above,
that's every node except the leaves of the tree.  After voting, the fullnode
prunes nodes that fork from a distance farther than the rollback depth and then
takes the opportunity to minimize its memory usage by squashing any nodes it
can into the root.

Starting from the example above, wth a rollback depth of 2, consider a vote on
5 versus a vote on 6. First, a vote on 5:

<img alt="Forks after pruning" src=".gitbook/assets/forks-pruned.svg" class="center"/>

The new root is 2, and any active forks that are not descendants from 2 are
pruned.

Alternatively, a vote on 6:

<img alt="Forks" src=".gitbook/assets/forks-pruned2.svg" class="center"/>

The tree remains with a root of 1, since the active fork starting at 6 is only
2 checkpoints from the root.
