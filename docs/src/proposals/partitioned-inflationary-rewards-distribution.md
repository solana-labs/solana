---
title: Partitioned Inflationary Rewards Distribution
---

## Problem

With the increase of number of stake accounts, computing and redeeming the stake
rewards at the start block of the epoch boundary becomes very expensive.
Currently, with 550K stake accounts, the stake reward time has already taken
more than 10 seconds. This prolonged computation slows down the network, and can
cause large number of forks at the epoch boundary, which makes the matter even
worse.

## Proposed Solutions

Instead of computing and reward stake accounts at epoch boundary, we will
decouple reward computation and reward credit into two phases.

A separate service, "EpochRewardCalculationService" will be created. The service
will listen to a channel for any incoming rewards calculation requests, and
perform the calculation for the rewards. For each block that cross the epoch
boundary, the bank will send a request to the `EpochRewardCalculationService`.
This marks the start of the reward computation phase.

```
N-1 -- N -- N+1
     \
      \
        N+2
```

In the above example, N is the start of the new epoch. Two rewards calculation
requests will be sent out at slot N and slot N+2 because they both cross the
epoch boundary and are on different forks. To avoid repeated computation with
the same input, the signature of the computation requests, `hash(epoch_number,
hash(stake_accounts_data), hash(vote_accounts), hash(delegation_map))`, are
calculated. Duplicated computation requests will be discard. For the above
example, if there are no stake/vote accounts changes between slot N and slot
N+2, the 2nd computation request will be discarded.

When reaching block height `N` after the start of the `reward computation
phase`, the bank starts the second phase - reward credit, in which, the bank
first query the `epoch calc service` with the request signature to get the
rewards result, which will be resented as a map from accounts_pubkey->rewards,
then credit the rewards to the stake accounts for the next `M` blocks. If the
rewards result is not available, the bank will wait until the results are
available.

We call them: <br/>
(a) calculating interval: `[epoch_start, epoch_start+N]` <br/>
(b) credit interval: `[epoch_start+N+1, epoch_start+N+M]`, respectively. <br/>
And the combined interval `[epoch_start, epoch_start+N+M]` is called
`rewarding interval`.

For `calculating interval`, `N` is chosen to be sufficiently large so that the
background computation should have completed and the result of the reward
computation is available at the end of `calculating interval`. `N` can be fixed
such as 100 (roughly equivalent to 50 seconds), or chosen as a function of the
number of stake accounts, `f(num_stake_accounts)`.

In `credit interval`,  the bank will fetch the reward computation results from
the background thread and start credit the rewards during the next `M` blocks.
The idea is partition the accounts into `M` partitions. And each block, the bank
credit `1/M` accounts. The partition is required to be deterministic for the
current epoch, but must also be random across different epochs. One way to
achieve these properties is to hash the account's pubkey with some epoch
dependent values, sort the results, and divide them into `M` bins. The epoch
dependent value can be the epoch number, total rewards for the epoch, the leader
pubkey for the epoch block, etc. `M` can be choses based on 50K account per
block, which equal to `ceil(num_stake_accounts/50,000)`.

`num_stake_account` is extracted from `leader_schedule_epoch` block, so we don't
run into discrepancy where new transactions right before an epoch boundary
creates one fork with `X` stake accounts and another fork with `Y` stake accounts.

In order to avoid putting extra burden of computing and credit the stake reward
for blocks produced during the `rewarding interval`, we can reduce the compute
budget limits on those blocks in `rewarding interval`, and reserve some computing
and read/write capacity to perform stake rewarding.

### Challenges

1. stake accounts reads/writes during the `rewarding interval`

`epoch_start..epoch_start+N+M` Because of the delayed credit of the rewards,
Reads to those stake accounts will not return the value that the user are
expecting (viz. not include the recent epoch stake rewards). Writes to those
stake accounts will be lost once the reward are credited on block
`epoch_start+N+M`. We will need to modify the runtime to restrict read/writes to
stake accounts during the `rewarding interval`. Any transactions, which involves
stake accounts, will result in a new execution error, i.e. "stake rewards
pending, account access is restricted". However, normal rpc queries, such as
'getBalance', will return the current lamport of the account. The user can
expect the rewards to be credit as some time point during the 'rewarding
interval'.

2. voting during `reward interval`

During reward interval, vote transactions must be processed normally for
achieving consensus and making progress for rooted blocks. However, those vote
transactions may potentially change the vote accounts balance (i.e. pay for the
voting transaction fee if vote_account and block reward recipient accounts
are the same), before the epoch rewards are paid. When the epoch rewards are
paid, those block rewards will be wiped out by the stale cached value. To
prevent this, we will enforce that the vote_account and authorized_voter
authority must be different.

3. snapshot taken during the `rewarding interval`

If a snapshot is taken during the `rewarding interval`, it would miss the
rewards for the stake accounts. Any plain restart from those snapshots will be
wrong, unless we reconstruct the rewards from the recent epoch boundary. This
will add some complexity to validator restart. In the first implementation, we
will force *not* taking any snapshot and *not* performing accounts hash
calculation during the `rewarding interval`. Incremental snapshot request will
be skipped. Full snapshot request will be re-queued be picked up later at the
end of the `reward interval`.

In future, if needed, we can
revisit to enable taking snapshots and perform hash calculation during reward
interval.

4. account-db related action during the `rewarding interval`

Account-db related action such as flush, clean, squash, shrink etc. may touch
and evict the stake accounts from account db's cache during the `rewarding
interval`. This will slow down the credit in the future at bank `epoch_start+N`.
We may need to exclude such accounts_db actions for stake_accounts during
`rewarding interval`. This is going to be a performance tuning problem. In the
first implementation, for simplicity, we will keep the account-db action as it
is, and make the `credit interval` larger to accommodate the performance hit
when writing back those accounts. In future, we can continue tuning account db
actions during 'rewarding interval'.

5. view of total epoch capitalization change

The view of total epoch capitalization, instead of being available at every
epoch boundary, is only available after the `rewarding interval`. Any third
party application logic, which depends on total epoch capitalization, need to
wait after `rewarding interval`.

6. `getInflationReward` JSONRPC API method call

Today, the `getInflationReward` JSONRPC API method call can simply grab the
first block in the target epoch and lookup the target stake account's rewards
entry.  With these changes, the call will need updated to derive the target
stake account's credit block, grab _that_ block, then lookup rewards.
Additionally we'll need to return more informative errors for queries made
during the lockout period, so users can know that their rewards are pending for
the target epoch. A new rpc API, i.e. `getRewardInterval`, will be added for
querying the `rewarding interval` for the current epoch.
