---
title: Fee Market for Storage
---

Safeguarding the cluster by imposing limits on the total size of accounts
data—whether this is per-transaction, per-block, or in-total—is arbitrary at
best, and unsafe at worst. Instead of relying on limits, a fee market on
storage can be implemented to add economic (dis)incentives for allocating
accounts data.


## Problems with Limits

Tracking and setting a cap for accounts data size—on transactions, blocks, and
in total—is tricky.


### Failing the transaction or the block

There are benefits to having different caps on allocated accounts data size
per-transaction, per-block, and in-total. But what happens in each case when
the cap is exceeded? If the per-transaction limit is exceeded, just that
transaction can be failed, which is good.

The per-block and in-total checks are different. There is not a single
transaction within the block that can be marked as _the one_ transaction that
caused the limit to be exceeded, as multiple transactions could exceed the
limit, and it would depend on how each validator executed the transactions
within the block. The solution then is fail the whole block. When a block is
failed, the downsides are (1) fees are not collected, and (2) votes do not
land. This would allow malicious actors to cause blocks to fail without paying
fees[^1]. This is bad. Failing the block for exceeding the in-total limit is
the same as well.

[^1] The malicious actors would still need sufficient SOL to create the
transactions and get the leader to put the transactions into a block, but if
the block later fails, those fees would not be deducted.


### TPU vs TVU

Validation of the accounts data size limits obviously occurs in the TVU. The
TPU should also have facilities so that it does not inadvertently create bad
blocks that later are dropped, which then causes the leader to not receive any
rewards for creating the block.

The TPU does not load programs/accounts, so transactions with CPI instructions
that cause allocations are invisible to the TPU. Currently, the TPU inspects
allocations if the instructions are `SystemInstruction`s as part of its cost
model. For users submitting transactions with large allocations, this
incentivizes them to create new programs that just wrap the System Program to
circumvent the TPU's cost model.

Without the TPU loading programs/accounts, how can it get sufficient insight
into all the instructions in all the transactions to ensure it does not create
a bad block that exceeds an accounts data size limit?


#### Transaction inclusion fairness

Even if the TPU could accurately track account data allocations for all
transactions, a per-block cap could incentivize malicious actors to create
low-cost transactions that use up the majority of the available accounts data
allocation space per block, causing other legitimate transactions-that-allocate
to not be included in the block.


## Proposed Solution

Auction the block's accounts data allocation space similar to how compute is
auctioned off. Transactions pay to allocate account data. Either a low limit is
set by default (maybe 1K), which enables small allocations, or no allocations
are allowed by default. Larger allocations must be paid for up-front, otherwise
the transaction fails.


## Open Questions

### Pricing

How do we price accounts data allocations? Should it be a fixed price, or be
open (on one side or both) to facilitate fee markets? I lean towards a minimum
amount per byte, but no maximum. Clients would submit their transaction's
allocation budget and fee, and the block producer can use that information when
packing blocks.

Should the fee scale based on the current total accounts data size? IOW, should
the per-byte fee increase as the total accounts data size increases?


### Limits

Even with this in place, should there still be limits? Either on
per-transaction, per-block, or in-total? My thought is to avoid hardcoded
limits where possible. Maybe with the scaling approach mentioned above, all
limits could be removed.


### Rent

How does this interact with rent? Can rent be removed entirely? Since
rent-paying accounts can no longer be created, validators do not end up earning
any lamports for rent on new accounts[^1]. Adding this new allocation fee would
be economically advantageous to validators.

[^1] Caveat: Pre-existing rent-paying accounts are grandfathered in, so some
rent collection fees are still collected by validators. Eventually all these
accounts with either be topped up to be rent-exempt, or drain to zero and go
away. At that point, validators will cease collecting any rent payments.
