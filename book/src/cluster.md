# A Solana Cluster

A Solana cluster is a set of fullnodes working together to serve client
transactions and maintain the integrity of the ledger. Many clusters may
coexist. When two clusters share a common genesis block, they attempt to
converge. Otherwise, they simply ignore the existence of the other.
Transactions sent to the wrong one are quietly rejected. In this chapter, we'll
discuss how a cluster is created, how nodes join the cluster, how they share
the ledger, how they ensure the ledger is replicated, and how they cope with
buggy and malicious nodes.

## Creating a Cluster

Before starting any fullnodes, one first needs to create a  *genesis block*.
The block contains entries referencing two public keys, a *mint* and a
*bootstrap leader*. The fullnode holding the bootstrap leader's secret key is
responsible for appending the first entries to the ledger. It initializes its
internal state with the mint's account. That account will hold the number of
native tokens defined by the genesis block. The second fullnode then contact
the bootstrap leader to register as a validator or replicator. Additional
fullnodes then register with any registered member of the cluster.

A validator receives all entries from the leader and is expected to submit
votes confirming those entries are valid. After voting, the validator is
expected to store those entries until *replicator* nodes submit proofs that
they have stored copies of it. Once the validator observes a sufficient number
of copies exist, it deletes its copy.

## Joining a Cluster

Fullnodes and replicators enter the cluster via registration messages sent to
its *control plane*. The control plane is implemented using a *gossip*
protocol, meaning that a node may register with any existing node, and expect
its registeration to propogate to all nodes in the cluster. The time it takes
for all nodes to synchonize is proportional to the square of the number of
nodes particating in the cluster. Algorithmically, that's considered very slow,
but in exchange for that time, a node is assured that it eventually has all the
same information as every other node, and that that information cannot be
censored by any one node.

## Ledger Broadcasting

A gossip network is much too slow to achieve subsecond finality once the
network grows beyond a certain size. The time it takes to send messages to all
nodes is proportional to the square of the number of nodes. If a blockchain
wants to achieve low finality and attempts to do it using a gossip network, it
will be forced to centralize to just a handful of nodes.  Solana believes that
being decentralized and permissionless are fundamental properties of a public
blockchain. If a blockchain wants to maintain subsecond finality, it must find
a solution that does not restrict the number of participants in the cluster.

Scalable finality requires a few tricks:

1. Break transactions up into small batches and hash each with a VDF.
2. Sign each batch with the leader's private key.
3. Structure nodes as a conceptual tree to efficiently propate the batch.

To hash the transaction batch, Solana uses its Proof of History VDF. Its hashes
tell validators that the transactions have not been reordered and what leader
slot they belong to.

Next, each batch is signed by the leader to ensure that a malicious leader is
not sending transactions beyond its allotted *slot*. When a validator verifies
the signature and the Proof of History hash, it can use the Proof of History
*tick height* to calculate the expected leader and reliably verify the
signature belongs to that leader.

Last, the fullnodes form a conceptual tree to efficiently pass each transaction
batch across a separate network called the *data plane*. The data plane
operates independently from the control plane described above, but can
physically reside on the same IP network. By forming a tree, data moves across
the network at a rate proportional to the height of the tree. That implies
finality times will increase with the logarithm of the number of nodes, where
the logarithm's base is the multiple of the number of nodes at each level.

<img alt="Data Plane Diagram" src="img/data-plane.svg" class="center"/>

In the diagram above, the multiple is 2 only because it is easiest to
visualize. In practice, the multiple is much heigher. Since network latency can
be higher than validation time, it's desirable to minimize the height of the
tree, which implies maximizing the multiple. However, increasing the multiple
means splitting the transaction batch into smaller pieces. When split so far
that the 32-byte Proof of History hash and 28 byte UDP/IP headers exceed its
size, the multiple is about as big as it's going to get.  We expect the
multiple to end up somewhere in the ballpark of 1,000 and that even a massive
worldwide cluster of fullnodes may have no more than three levels.

At the time of this writing, the 150 validator testnet is just two levels: the
leader on one and all validators on the other. We anticipate adding a third
level soon to serve replicator nodes, which the cluster permits to have high
network latency. Because of that additional latency, they cannot sit in the
second level, where quickly sharing data with their peers is required to reach
finality.

## Malicious Nodes

Solana is a *permissionless* blockchain, meaning that anyone wanting to
participate in the network may do so. They need only *stake* some
cluster-defined number of tokens and be willing to lose that stake if the
cluster observes the node acting maliciously. The process is called *Proof of
Stake* consensus, which defines rules to *slash* the stakes of malicious nodes.
