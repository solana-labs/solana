# Fork Checkpoints

This design describes the software architecture for fork check pointing.  It addresses the following challenges:

* Forks are created at every block.
* Forks can be based on any previously produced block.
* Forks are eventually fully committed such that rollback is impossible.
* Unreachable forks need to be pruned.

## Architecture

The basic design idea is to maintain a DAG of forks.  Each fork points back to a single ancestor.  The DAG is initialized with a root.  Each subsequent fork must descend from the root.  

## Active Chains

An *active chain* is a direct list of connected forks that descend from the current root to a specific fork without any descendants.

For example:
```
      1
     / \
    2   \
   /|   |
  / |   |
 4  |   |
    5   /\  
       6  \ 
           7
```

The following *acitve chains* are in the checkpoints DAG

* 4,2,1
* 5,2,1
* 6,1
* 7,1

## Merging into root

A validator votes for a finalized fork.  The *active chain* connecting the fork to the root is merged.  If the *active chain* is longer than `Forks::ROLLBACK_DEPTH` the oldest two forks are merged.  The oldest fork in the *active chain* is the current root, so the second oldest is a direct descendant of the root fork.  Once merged, the current root is updated to the root descendant. Any forks that are not descendants from the new root are pruned since they are no longer reachable.

For example:
```
      1
     / \
    2   \
   /|   |
  / |   |
 4  |   |
    5   /\  
       6  \ 
           7
```

* ROLLBACK\_DEPTH=2, vote=5, *active chain*={5,2,1}

```
    2 
   /| 
  / | 
 4  | 
    5 
      
```

The new root is 2, and any active chains that are not descendants from 2 are pruned.

* ROLLBACK\_DEPTH=2, vote=6, *active chain*={6,1}

```
      1
     / \
    2   \
   /|   |
  / |   |
 4  |   |
    5   /\  
       6  \ 
           7 
```

The tree remains with `root=1`, since the *active chain* starting at 6 is only 2 forks long.
