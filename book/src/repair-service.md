# Repair Service

The Repair Service is in charge of retrieving missing blobs that failed to be delivered by primary communication protocols like Avalanche. It is also the primary mechanism by which new nodes joining the cluster fetch old portions of the ledger.

# Repair Protocol

The repair protocol makes best attempts to progress the forking structure of Blocktree. Blocktree makes available a `slots_of_interest` field, which is a vector of (weight, slot) tuples, which represent the weighted value of a possible fork point, as most recently calculated by Locktower. `slots_of_interest` should be sorted by weight.

The protocol:

1. Local-Weighted Repair: 
    Problem: Slots can be missing blobs as a result of transmission failures. The goal of this phase is to repair missing blobs within slots in Blocktree that are known to chain to some fork in `slots_of_interest`. 
    
    Proposal: Iterate through `slots_of_interest` in order of fork weight. For each fork F in `slots_of_interest`, repair all missing blobs in slots S that meet the criteria:
    * S <= current local Poh slot height 
    * S is connected to F, i.e. S is discoverable by searching the subtree of children rooted at F.


We limit this phase of the protocol to generate at most some maximum `N` requests.

2. Cluster-Weighted Repair: 
    Problem: After a long partition, there can possibly be important forks that don't have a lot of weight on the local node (for instance, maybe a node only has the beginning of such a fork). Local-Weighted Repair won't repair these forks.

    Proposal: TBD. Maybe repair these forks based on the number of votes in gossip?

3. Preemptive Repair: 
    Problem: Consider if `slots_of_interest` contains the set of forks {1, 3, 5}. Then Blocktree receives blobs for some slot 7, where for each of the blobs b, b.parent == 6, so the parent-child relation 6 -> 7 is stored in blocktree. However, there is no way to chain these slots to any of the forks in `slots_of_interest`, and thus the first phase of `Weighted Repair` will not repair these slots. If these slots are part of the main chain of, this will halt replay progress on this node.

    Proposal: The goal of this phase is to discover the chaining relationship of isolated slots that do not currently chain to any known fork. 

    Pre-work: 
        * Introduce a new structure in blocktree that tracks slots that do not have a known parent. In the problem description above, slot 6 would be an example of such a slot.

        * Introduce a flag `chains_to_root` on `SlotMeta` in Blocktree, which is set to true on some `SlotMeta` `S` if there is a path from `S` to slot 0 by traversing the parents.

        * Modify blob transmission protocol such that the first blob in each slot contains
        some large `N` number of its previous parents.
    

    For each isolated slot S:
        Ask for blob 0 of `S` from peers. Once received, we will know the set of `parents`: a vector of the previous `N` parent slots of `S`. Then for each `p` in `parents`:
            * Insert an empty `SlotMeta` for `p` if it doesn't already exist.
            * If `p` does exist, update the parent of `p` based on `parents`
            * Terminate if the Parent(p).chains_to_root == true
    
        Note that once these empty slots are added to blocktree, cluster-weighted repair should attempt to fill those slots if they are on an important fork.
