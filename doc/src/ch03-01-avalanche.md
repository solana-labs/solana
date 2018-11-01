# Avalanche replication

The [Avalance explainer video](https://www.youtube.com/watch?v=qt_gDRXHrHQ) is
a conceptual overview of how a Solana leader can continuously process a gigabit
of transaction data per second and then get that same data, after being
recorded on the ledger, out to multiple validators on a single gigabit
backplane.

In practice, we found that just one level of the Avalanche validator tree is
sufficient for at least 150 validators. We anticipate adding the second level
to solve one of two problems:

1. To transmit ledger segments to slower "replicator" nodes.
2. To scale up the number of validators nodes.

Both problems justify the additional level, but you won't find it implemented
in the reference design just yet, because Solana's gossip implementation is
currently the bottleneck on the number of nodes per Solana cluster.  That work
is being actively developed here:

[Scalable Gossip](https://github.com/solana-labs/solana/pull/1546)

