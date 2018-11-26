# A Solana Cluster

A Solana cluster is a set of fullnodes working together to serve client
transactions and maintain the integrity of the ledger. Many clusters may
coexist.  When two clusters share a common genesis block, they attempt to
converge. Otherwise, they simply ignore the existence of the other.
Transactions sent to the wrong one are quietly rejected. In this chapter,
we'll discuss how a cluster is created, how nodes join the cluster, how
they share the ledger, how they ensure the ledger is replicated, and how
the cope with buggy and malicious nodes.
