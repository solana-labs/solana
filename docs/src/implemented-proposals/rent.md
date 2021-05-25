---
title: Rent
---

Accounts on Solana may have owner-controlled state \(`Account::data`\) that's separate from the account's balance \(`Account::lamports`\). Since validators on the network need to maintain a working copy of this state in memory, the network charges a time-and-space based fee for this resource consumption, also known as Rent.

## Two-tiered rent regime

Accounts which maintain a minimum balance equivalent to 2 years of rent payments are exempt. The _2 years_ is drawn from the fact hardware cost drops by 50% in price every 2 years and the resulting convergence due to being a geometric series. Accounts whose balance falls below this threshold are charged rent at a rate specified in genesis, in lamports per byte-year. The network charges rent on a per-epoch basis, in credit for the next epoch, and `Account::rent_epoch` keeps track of the next time rent should be collected from the account.

Currently, the rent cost is fixed at the genesis. However, it's anticipated to be dynamic, reflecting the underlying hardware storage cost at the time. So the price is generally expected to decrease as the hardware cost declines as the technology advances.

## Timings of collecting rent

There are two timings of collecting rent from accounts: \(1\) when referenced by a transaction, \(2\) periodically once an epoch. \(1\) includes the transaction to create the new account itself, and it happens during the normal transaction processing by the bank as part of the load phase. \(2\) exists to ensure to collect rents from stale accounts, which aren't referenced in recent epochs at all. \(2\) requires the whole scan of accounts and is spread over an epoch based on account address prefix to avoid load spikes due to this rent collection.

On the contrary, rent collection isn't applied to accounts that are directly manipulated by any of protocol-level bookkeeping processes including:

- The distribution of rent collection itself (Otherwise, it may cause recursive rent collection handling)
- The distribution of staking rewards at the start of every epoch (To reduce as much as processing spike at the start of new epoch)
- The distribution of transaction fee at the end of every slot

Even if those processes are out of scope of rent collection, all of manipulated accounts will eventually be handled by the \(2\) mechanism.

## Actual processing of collecting rent

Rent is due for one epoch's worth of time, and accounts have `Account::rent_epoch` of `current_epoch` or `current_epoch + 1` depending on the rent regime.

If the account is in the exempt regime, `Account::rent_epoch` is simply updated to `current_epoch`.

If the account is non-exempt, the difference between the next epoch and `Account::rent_epoch` is used to calculate the amount of rent owed by this account \(via `Rent::due()`\). Any fractional lamports of the calculation are truncated. Rent due is deducted from `Account::lamports` and `Account::rent_epoch` is updated to `current_epoch + 1` (= next epoch). If the amount of rent due is less than one lamport, no changes are made to the account.

Accounts whose balance is insufficient to satisfy the rent that would be due simply fail to load.

A percentage of the rent collected is destroyed. The rest is distributed to validator accounts by stake weight, a la transaction fees, at the end of every slot.

Finally, rent collection happens according to the protocol-level account updates like the rent distribution to validators, meaning there is no corresponding transaction for rent deductions. So, rent collection is rather invisible, only implicitly observable by a recent transaction or predetermined timing given its account address prefix.

## Design considerations

### Current design rationale

Under the preceding design, it is NOT possible to have accounts that linger, never get touched, and never have to pay rent. Accounts always pay rent exactly once for each epoch, except rent-exempt, sysvar and executable accounts.

This is an intended design choice. Otherwise, it would be possible to trigger unauthorized rent collection with `Noop` instruction by anyone who may unfairly profit from the rent (a leader at the moment) or save the rent given anticipated fluctuating rent cost.

As another side-effect of this choice, also note that this periodic rent collection effectively forces validators not to store stale accounts into a cold storage optimistically and save the storage cost, which is unfavorable for account owners and may cause transactions on them to stall longer than others. On the flip side, this prevents malicious users from creating significant numbers of garbage accounts, burdening validators.

As the overall consequence of this design, all accounts are stored equally as a validator's working set with the same performance characteristics, reflecting the uniform rent pricing structure.

### Ad-hoc collection

Collecting rent on an as-needed basis \(i.e. whenever accounts were loaded/accessed\) was considered. The issues with such an approach are:

- accounts loaded as "credit only" for a transaction could very reasonably be expected to have rent due,

  but would not be writable during any such transaction

- a mechanism to "beat the bushes" \(i.e. go find accounts that need to pay rent\) is desirable,

  lest accounts that are loaded infrequently get a free ride

### System instruction for collecting rent

Collecting rent via a system instruction was considered, as it would naturally have distributed rent to active and stake-weighted nodes and could have been done incrementally. However:

- it would have adversely affected network throughput
- it would require special-casing by the runtime, as accounts with non-SystemProgram owners may be debited by this instruction
- someone would have to issue the transactions
