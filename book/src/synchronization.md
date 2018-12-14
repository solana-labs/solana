# Synchronization

Fast, reliable synchronization is the biggest reason Solana is able to achieve
such high throughput. Traditional blockchains synchronize on large chunks of
transactions called blocks. By synchronizing on blocks, a transaction cannot
be confirmed until a duration called "block time" has passed. In Proof of Work
consensus, these block times need to be very large (~10 minutes) to minimize
the odds of multiple fullnodes producing a new valid block at the same time.

Block times can be much shorter with Proof of Stake consensus, but without a
reliable clock to synchronize on, a fullnode cannot distinguish an out-of-order
block from an invalid block. Both will point to a previous block with a hash
the validator hasn't seen. The popular workaround is to tag each block with a
"median timestamp". All nodes share their wallclock times with each other and
the median is what is used as the timestamp. If a node with substantial clock
drift goes offline, the median timestamp will suddenly jump. To account for
that, those systems require long block times such that a block doesn't include
a timestamp before the previous block.

Solana takes a very different approach, which it calls *Proof of History* or
*PoH*. Leader nodes "timestamp" blocks with cryptographic proofs that some
duration of time has passed since the last proof. All data hashed into the
proof most certainly have occurred before the proof was generated. The node
then shares the new block with validator nodes, which are able to verify those
proofs. The blocks can arrive at validators in any order or even could be
replayed years later. With such reliable synchronization guarantees, Solana is
able to break blocks into smaller batches of transactions called *entries*.
Entries are streamed to validators in realtime, before any notion of block
consensus.

Solana reserves the term *block* for a sequence of entries that fullnodes cast
votes on to achieve *confirmation*. Because the entries are streamed to
validators, transactions are processed long before it is time to vote. There is
effectively no delay between the time the last entry is received and the time
when the node can vote. And because of the timestamps, any time consensus is
**not** achieved, a node simply rolls back its state. This optimisic processing
technique was introduced in 1981 and called Optimistic Concurrency Control.
[\[H.T.Kung, J.T.Robinson
(1981)\]](http://citeseerx.ist.psu.edu/viewdoc/summary?doi=10.1.1.65.4735)

### Relationship to VDFs

The Proof of History technique was first described for use in blockchain by
Solana in November of 2017. In June of the following year, a similar technique
was described at Stanford and called a [verifiable delay
function](https://eprint.iacr.org/2018/601.pdf) or *VDF*.

A desirable property of a VDF is that verification time is very fast. Solana's
approach to verifying its delay function is proportional to the time it took to
create it. Split over a 4000 core GPU, it is sufficiently fast for Solana's
needs, but if you asked the authors the paper cited above, they might tell you
([and have](https://github.com/solana-labs/solana/issues/388)) that Solana's
approach is algorithmically slow it shouldn't be called a VDF. We argue the
term VDF should represent the category of verifiable delay functions and not
just the subset with certain performance characteristics. Until that's
resolved, Solana will likely continue using the term PoH for its
application-specific VDF.

Another difference between PoH and VDFs is that a VDF is used only for tracking
duration. PoH's hash chain, on the other hand, includes hashes of any data the
application observed.  That data is a double-edged sword. On one side, the data
"proves history" - that the data most certainly existed before hashes after it.
On the side, it means the application can manipulate the hash chain by changing
*when* the data is hashed. The PoH chain therefore does not serve as a good
source of randomness whereas a VDF without that data could. Solana's [leader
rotation algorithm](#leader-rotation), for example, is derived only from the
VDF *height* and not its hash at that height.

### Relationship to Consensus Mechanisms

Proof of History is not a consensus mechanism, but it is used to improve the
performance of Solana's Proof of Stake consensus. It is also used to improve
the performance of the data plane and replication protocols.

### More on Proof of History

* [water clock
  analogy](https://medium.com/solana-labs/proof-of-history-explained-by-a-water-clock-e682183417b8)

* [Proof of History
  overview](https://medium.com/solana-labs/proof-of-history-a-clock-for-blockchain-cf47a61a9274)

