---
title: Fee Model
---

Fee Model provides a solution to prioritize access to shared state.


## Problem

Fixed low transaction fee does not disincentivize bots from spamming the network
in order to compete for blockspace; users are not able to land transactions that
do not access contended state when network is congested and leader is overloaded.


## Proposed solution

The fee model provides a solution to land transactions even when leaders are
loaded or required state are highly contended by building a local fee market
where hotspots are priced dynamically.

[Prioritization fee](https://github.com/solana-labs/solana/blob/master/docs/src/terminology.md#prioritization-fee)
enables users to have their transactions prioritized over others that compete
on the same state. All other transactions not accessing that state would be
processed as normal without priority fee when there are available blockspace.
This is different from global fee mechanism where everyone have to pay higher
fees when congested.

When there are more transactions than there is blockspace, users can raise
prioritization fee to land their transactions.

Network is prioritizing across banking and forwarding queue by account write
compute-unit limits and block compute-unit limits, economically incentivize
transactions to be more efficient (e.g., low required compute-units) and
pays resources properly.

Users can query RPC for a list of minimum prioritization fees from recent
blocks to estimate what to pay to get a necessary transaction included in block.


### Set Prioritization Fee

Transactions are prioritized by compute-unit price (specified in increments of
micro-lamports per compute-unit). It is set by Compute Budget instructions
`SetComputeUnitPrice`.

Prioritization fee is calculated by multiplying the requested maximum compute
units by the compute-unit price. The fee is collected as part of standard
transaction fee, half of it is distributed to the leader, another half is burned.


### Prioritize transactions at banking queue

Leader sorts received transactions by compute-unit price in descending order in
it's banking queue. When it starts to produce blocks, the highest priority
transaction (e.g., highest compute-unit price) will be scheduled first.

Scheduled transaction consumes compute-unit from block limit and accounts write
compute-unit limit; however if accounts the transaction required have reached
limits, or block has reached limits, it is put back to queue for later retry.

Process continues on to the next higher priority transaction in queue to fill up
account buckets and block.

This ensures transactions not require contended accounts will still be included
in block with lower priority fee.


### Prioritize transactions at forwarding queue

Forwarding queue is organized as a batch of virtual blocks, with pre-determined
block limit and write account limit. Forwardable transactions are filled into
virtual blocks, following the same rules as leader packs transactions into block.

If an account reached limits in all virtual blocks, forwardable transactions
require that account will no longer be accepted for forwarding, while the other
transactions can still be accepted.

Virtual blocks with multiple account buckets are forwarded to next leader,
avoiding starving accounts on forwarding.


### Fee Market

Validator caches recent blocks prioritization-fee data during replay. Users can
access the cache via RPC method [getRecentPrioritizationFees](https://github.com/solana-labs/solana/blob/master/docs/src/developing/clients/jsonrpc-api.md#getrecentprioritizationfees) to get a list of minimum prioritization fee to
land a transaction locking all required accounts from recent blocks. Users
therefore can get estimate of what fee is necessary to get their transaction
included.


### Cli supports Prioritization Fee

Cli can prioritize transactions it sends with option `--with-compute-unit-price`


