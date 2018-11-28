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

To create a cluster, one needs the fullnode software and a *genesis block*. A
minimal genesis block can be created using the command-line tools
`solana-keygen`, `solana-fullnode-config` and `solana-genesis`.
`solana-keygen` is used to generate two keypairs, one for the *Mint* and one
for the *Bootstrap Leader*. `solana-fullnode-config` is used to pin down what
IP addresses and ports other nodes should use to contact it. Next, the
information from the first two tools are passed to `solana-genesis` to create
the genesis block. The genesis block is then be passed to all initial
fullnodes. The fullnode holding the Bootstrap Leader's secret key is
responsible for appending the first entries to the ledger. It initializes its
internal state with the Mint's account. That account will hold the number of
fractional native tokens (called lamports) defined in the genesis block. All
other fullnodes should then contact the Bootstrap Leader and register as a
validator or replicator. A validator receives all entries from the leader and
is expected to submit votes confirming those entries are valid. After voting,
the validator is expected to store those entries until *replicator* nodes
submit proofs that they have stored copies of it. Once the validator observes a
sufficient number of copies exist, it deletes its copy.

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

The [Avalance explainer video](https://www.youtube.com/watch?v=qt_gDRXHrHQ) is
a conceptual overview of how a Solana leader can continuously process a gigabit
of transaction data per second and then get that same data, after being
recorded on the ledger, out to multiple validators on a single gigabit
*data plane*.

In practice, we found that just one level of the Avalanche validator tree is
sufficient for at least 150 validators. We anticipate adding the second level
to solve one of two problems:

1. To transmit ledger segments to slower "replicator" nodes.
2. To scale up the number of validators nodes.

Both problems justify the additional level, but you won't find it implemented
in the reference design just yet, because Solana's gossip implementation is
currently the bottleneck on the number of nodes per Solana cluster.

## Malicious Nodes

Solana is a *permissionless* blockchain, meaning that anyone wanting to
participate in the network may do so. They need only *stake* some
cluster-defined number of tokens and be willing to lose that stake if the
cluster observes the node acting maliciously. The process is called *Proof of
Stake* consensus, which defines rules to *slash* the stakes of malicious nodes.
