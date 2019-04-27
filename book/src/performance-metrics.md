# Performance Metrics

Solana cluster performance is measured as average number of transactions per second
that the network can sustain (TPS). And, how long it takes for a transcation to be
confirmed by super majoring of the cluster (Confirmation Time).

Each cluster node maintains various counters that are incremented on certain events.
These counters are periodically uploaded to a cloud based database. Solana's metrics
dashboard fetches these counters, and computes the performance metrics and displays
it on the dashboard. 

## TPS

The leader node's bank maintains a count of transactions that it processed. The dashboard
displays sum of the count for every 2 second period in the TPS time series graph.
The dashboard also shows mean, maximum and total TPS as a running counter.

## Confimation Time

Each validator node maintains a list of active ledger forks that are visible
to the node. Whenever a fork's final tick is registered, the fork is frozen, and
considered to be confirmed. The node assigns a timestamp to every new fork, and
computes the time it took to confirm the fork. This time is reflected as validator
confirmation time in performance metrics. The performance dashboard displays the
average of each validator node's confirmation time as a time series graph. 