# Fork Deltas

This design describes a way to checkpoint the bank state such that it can track multiple forks without duplicating data.  It addresses the following challenges:

* Forks are potentially created at every slot boundary.
* Forks can be based on any previously produced block.
* Forks are eventually finalized such that rollback is impossible.
* Unreachable forks need to be pruned.

## Architecture

The basic design idea is to maintain a DAG of forks.  Each fork points back to a single ancestor.  The DAG is initialized with a root.  Each subsequent fork must descend from the root.

## Active Forks

An *active fork* is a direct list of connected forks that descend from the current root to a specific fork without any descendants.

For example:

<img alt="Forks" src="img/forks.svg" class="center"/>

The following *active forks* are in the deltas DAG

* 4,2,1
* 5,2,1
* 6,1
* 7,1

## Merging into root

A validator votes for a finalized fork.  The *active fork* connecting the fork to the root is merged.  If the *active fork* is longer than `Forks::ROLLBACK_DEPTH` the oldest two forks are merged.  The oldest fork in the *active fork* is the current root, so the second oldest is a direct descendant of the root fork.  Once merged, the current root is updated to the root descendant. Any forks that are not descendants from the new root are pruned since they are no longer reachable.

For example:

<img alt="Forks" src="img/forks.svg" class="center"/>

* ROLLBACK\_DEPTH=2, vote=5, *active fork*={5,2,1}

<img alt="Forks after pruning" src="img/forks-pruned.svg" class="center"/>

The new root is 2, and any active forks that are not descendants from 2 are pruned.

* ROLLBACK\_DEPTH=2, vote=6, *active fork*={6,1}

<img alt="Forks" src="img/forks.svg" class="center"/>

The tree remains with `root=1`, since the *active fork* starting at 6 is only 2 forks long.
