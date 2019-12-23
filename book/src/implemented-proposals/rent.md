# Rent

Accounts on Solana may have owner-controlled state \(`Account::data`\) that's separate from the account's balance \(`Account::lamports`\). Since validators on the network need to maintain a working copy of this state in memory, the network charges a time-and-space based fee for this resource consumption, also known as Rent.

## Two-tiered rent regime

Accounts which maintain a minimum balance equivalent to 2 years of rent payments are exempt. Accounts whose balance falls below this threshold are charged rent at a rate specified in genesis, in lamports per kilobyte-year. The network charges rent on a per-epoch basis, in credit for the next epoch \(but in arrears when necessary\), and `Account::rent_epoch` keeps track of the next time rent should be collected from the account.

## Collecting rent

Rent is due at account creation time for one epoch's worth of time, and the new account has `Account::rent_epoch` of `current_epoch + 1`. After that, the bank deducts rent from accounts during normal transaction processing as part of the load phase.

If the account is in the exempt regime, `Account::rent_epoch` is simply pushed to `current_epoch + 1`.

If the account is non-exempt, the difference between the next epoch and `Account::rent_epoch` is used to calculate the amount of rent owed by this account \(via `Rent::due()`\). Any fractional lamports of the calculation are truncated. Rent due is deducted from `Account::lamports` and `Account::rent_epoch` is updated to the next epoch. If the amount of rent due is less than one lamport, no changes are made to the account.

Accounts whose balance is insufficient to satisfy the rent that would be due simply fail to load.

A percentage of the rent collected is destroyed. The rest is distributed to validator accounts by stake weight, a la transaction fees, at the end of every slot.

## Read-only accounts

Read-only accounts are not being charged rent in current implementation.

## Design considerations, others considered

Under this design, it is possible to have accounts that linger, never get touched, and never have to pay rent. `Noop` instructions that name these accounts can be used to "garbage collect", but it'd also be possible for accounts that never get touched to migrate out of a validator's working set, thereby reducing memory consumption and obviating the need to charge rent.

### Ad-hoc collection

Collecting rent on an as-needed basis \(i.e. whenever accounts were loaded/accessed\) was considered. The issues with such an approach are:

* accounts loaded as "credit only" for a transaction could very reasonably be expected to have rent due,

  but would not be writable during any such transaction

* a mechanism to "beat the bushes" \(i.e. go find accounts that need to pay rent\) is desirable,

  lest accounts that are loaded infrequently get a free ride

### System instruction for collecting rent

Collecting rent via a system instruction was considered, as it would naturally have distributed rent to active and stake-weighted nodes and could have been done incrementally. However:

* it would have adversely affected network throughput
* it would require special-casing by the runtime, as accounts with non-SystemProgram owners may be debited by this instruction
* someone would have to issue the transactions

### Account scans on every epoch

Scanning the entire Bank for accounts that owe rent at the beginning of each epoch was considered. This would have been an expensive operation, and would require that the entire current state of the network be present on every validator at the beginning of each epoch.

