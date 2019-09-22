# Performance Metrics

Solana cluster performance is measured as average number of transactions per second
that the network can sustain (TPS). And, how long it takes for a transaction to be
confirmed by super majority of the cluster (Confirmation Time).

Each cluster node maintains various counters that are incremented on certain events.
These counters are periodically uploaded to a cloud based database. Solana's metrics
dashboard fetches these counters, and computes the performance metrics and displays
it on the dashboard.

## TPS

Each node's bank runtime maintains a count of transactions that it has processed.
The dashboard first calculates the median count of transactions across all metrics
enabled nodes in the cluster. The median cluster transaction count is then averaged
over a 2 second period and displayed in the TPS time series graph. The dashboard also
shows the Mean TPS, Max TPS and Total Transaction Count stats which are all calculated from
the median transaction count.

## Confirmation Time

Each validator node maintains a list of active ledger forks that are visible to the node.
A fork is considered to be frozen when the node has received and processed all entries
corresponding to the fork. A fork is considered to be confirmed when it receives cumulative
super majority vote, and when one of its children forks is frozen.

The node assigns a timestamp to every new fork, and computes the time it took to confirm
the fork. This time is reflected as validator confirmation time in performance metrics.
The performance dashboard displays the average of each validator node's confirmation time
as a time series graph.
