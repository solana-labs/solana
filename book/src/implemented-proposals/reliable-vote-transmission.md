# Reliable Vote Transmission

Validator votes are messages that have a critical function for consensus and continuous operation of the network. Therefore it is critical that they are reliably delivered and encoded into the ledger.

## Challenges

1. Leader rotation is triggered by PoH, which is clock with high drift. So many nodes are likely to have an incorrect view if the next leader is active in realtime or not.
2. The next leader may be easily be flooded. Thus a DDOS would not only prevent delivery of regular transactions, but also consensus messages.
3. UDP is unreliable, and our asynchronous protocol requires any message that is transmitted to be retransmitted until it is observed in the ledger. Retransmittion could potentially cause an unintentional _thundering herd_ against the leader with a large number of validators. Worst case flood would be `(num_nodes * num_retransmits)`.
4. Tracking if the vote has been transmitted or not via the ledger does not guarantee it will appear in a confirmed block. The current observed block may be unrolled. Validators would need to maintain state for each vote and fork.

## Design

1. Send votes as a push message through gossip. This ensures delivery of the vote to all the next leaders, not just the next future one.
2. Leaders will read the Crds table for new votes and encode any new received votes into the blocks they propose. This allows for validator votes to be included in rollback forks by all the future leaders.
3. Validators that receive votes in the ledger will add them to their local crds table, not as a push request, but simply add them to the table. This shortcuts the push message protocol, so the validation messages do not need to be retransmitted twice around the network.
4. CrdsValue for vote should look like this `Votes(Vec<Transaction>)`

Each vote transaction should maintain a `wallclock` in its data. The merge strategy for Votes will keep the last N set of votes as configured by the local client. For push/pull the vector is traversed recursively and each Transaction is treated as an individual CrdsValue with its own local wallclock and signature.

Gossip is designed for efficient propagation of state. Messages that are sent through gossip-push are batched and propagated with a minimum spanning tree to the rest of the network. Any partial failures in the tree are actively repaired with the gossip-pull protocol while minimizing the amount of data transfered between any nodes.

## How this design solves the Challenges

1. Because there is no easy way for validators to be in sync with leaders on the leader's "active" state, gossip allows for eventual delivery regardless of that state.
2. Gossip will deliver the messages to all the subsequent leaders, so if the current leader is flooded the next leader would have already received these votes and is able to encode them.
3. Gossip minimizes the number of requests through the network by maintaining an efficient spanning tree, and using bloom filters to repair state. So retransmit back-off is not necessary and messages are batched.
4. Leaders that read the crds table for votes will encode all the new valid votes that appear in the table. Even if this leader's block is unrolled, the next leader will try to add the same votes without any additional work done by the validator. Thus ensuring not only eventual delivery, but eventual encoding into the ledger.

## Performance

1. Worst case propagation time to the next leader is Log\(N\) hops with a base depending on the fanout. With our current default fanout of 6, it is about 6 hops to 20k nodes.
2. The leader should receive 20k validation votes aggregated by gossip-push into MTU-sized shreds. Which would reduce the number of packets for 20k network to 80 shreds.
3. Each validators votes is replicated across the entire network. To maintain a queue of 5 previous votes the Crds table would grow by 25 megabytes. `(20,000 nodes * 256 bytes * 5)`.

## Two step implementation rollout

Initially the network can perform reliably with just 1 vote transmitted and maintained through the network with the current Vote implementation. For small networks a fanout of 6 is sufficient. With small network the memory and push overhead is minor.

### Sub 1k validator network

1. Crds just maintains the validators latest vote.
2. Votes are pushed and retransmitted regardless if they are appearing in the ledger.
3. Fanout of 6.
4. Worst case 256kb memory overhead per node.
5. Worst case 4 hops to propagate to every node.
6. Leader should receive the entire validator vote set in 4 push message shreds.

### Sub 20k network

Everything above plus the following:

1. CRDS table maintains a vector of 5 latest validator votes.
2. Votes encode a wallclock. CrdsValue::Votes is a type that recurses into the transaction vector for all the gossip protocols.
3. Increase fanout to 20.
4. Worst case 25mb memory overhead per node.
5. Sub 4 hops worst case to deliver to the entire network.
6. 80 shreds received by the leader for all the validator messages.

