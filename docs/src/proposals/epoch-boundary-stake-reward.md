---
title: Partitioned Inflationary Rewards Distribution
---

## Problem

With the increase of number of stake accounts, computing and redeeming the stake
rewards at the start slot of the epoch boundary becomes very expensive.
Currently, with 550K stake accounts, the stake reward time has already taken
more than 10 seconds. This prolonged computation slows down the network, and can
cause large number of forks at the epoch boundary, which make the matter even
worse.

## Proposed Solutions

Instead of computing and reward stake accounts at epoch boundary, we will
decouple reward computation and reward credit into two phases. At the start of
the new epoch, a background thread is forked to compute the rewards. This is the
first phase - reward computation. When reaching slot `N` after the epoch, where
`N` is the first slot on its fork greater than `K`, then the bank starts of the
second phase - reward credit, in which the rewards will be credited to all the
stake accounts, which will last for `M` slots.

We call these two phases `epoch_start..epoch_start+N..epoch_start+N+M` slots are
`rewarding interval`. In the first phase, `K` is chosen to be sufficiently large
so that the background computation should have completed and the result of the
reward computation is available at the end of `rewarding interval`. `K` can be
fixed such as 100 (roughly equivalent to 50 seconds), or chosen as a function of
the number of stake accounts, `f(num_stake_accounts)`. In the second phase, when
reached slot `N`, the bank will fetch the reward computation results from the
background thread and start credit the rewards during the next `M` slots. The
idea is partition the accounts into `M` partitions. And each slot, the bank
credit `1/M` accounts. The partition is required to be deterministic for the
current epoch, but must also be random across different epochs. One way to
achieve these property of the partitions is to use some epoch dependent values
to hash with the account pubkey, sort the results and divide them into `1/M`
bins. The epoch dependent value can be the epoch number, or total rewards for
the epoch, or the leader pubkey for slot `N`. `M` can be choses based on 50K
account per slot, which is `ceil(num_stake_accounts/50,000)`.

### Challenges

1. stake accounts reads/writes during the `rewarding interval`

`epoch_start..epoch_start+N+M` Because of the delayed credit of the rewards,
Reads to those stake accounts may not will return the value that the user are
expecting (viz. not include the recent epoch stake rewards). Writes to those
stake accounts will be lost once the reward are credited on slot
`epoch_start+N+M`. We may need to modify the runtime to restrict read/writes to
stake accounts during the `rewarding interval`.

2. snapshot taken during the `rewarding interval`

If a snapshot is taken during the `rewarding interval`, it would miss the
rewards for the stake accounts. Any plain restart from those snapshots will be
wrong, unless we reconstruct the rewards from the recent epoch boundary. This
will add much more complexity to validator restart. Alternatively, we can force
not taking any snapshot during the `rewarding interval`.

3. account-db related action during the `rewarding interval`

Account-db related action such as flush, clean, squash, shrink etc. may touch
and evict the stake accounts from account db's cache during the `rewarding
interval`. This will slow down the credit in the future at slot `epoch_start+N`.
We will need to exclude such account_db actions for stake_accounts during
`rewarding interval`.

4. view of total epoch capitalization change

The view of total epoch capitalization, instead of being available at every
epoch boundary, is only available after the `rewarding interval`. Any logic that
depends on total epoch capitalization need to wait after `rewarding interval`.