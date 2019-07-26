# Rent

Accounts on Solana may have owner-controlled state (`Account::data`) that's separate 
from the account's balance (`Account::lamports`).  Since validators on the network need 
to maintain a working copy of this state in memory, the network charges a time-and-space 
based fee for this resource consumption, also known as Rent.

## Two-tiered rent regime

Accounts which maintain a minimum balance equivalent to 2 years of rent are exempt from
rent.  Accounts whose balance falls below this threshold are charged rent 
at a rate specified in genesis, in lamports per kilobyte-year.

## Collecting rent

Rent is charged at the end of each slot on all accounts that were accessed during that block.  A field in the account `Account::rent_slot` is keeps track of the last time rent was paid by this account.

If the account is in the exempt rent regime, `Account::rent_slot` is simply updated to the current slot.

If the account not exempt, the difference between the current slot and `rent_slot` is used to calculate the number of years since the last rent payment, which is passed to `Rent::due()` to calculate the amount of rent owed by this account.  Any fractional lamports are truncated.  Rent is collected from the account and `Account::rent_slot` is updated to the current slot.

If the amount of rent due is less than one lamport, no rent is collected and `Account::rent_slot` is not updated.

`Rent::burn_percent` of the rent collected is destroyed, the rest is distributed to validator accounts by stake weight, a la transaction fees.  Any fractional lamports are destroyed.

## Deliquency

Accounts whose balance reaches zero due to rent collection are dropped.

## Design considerations, others considered

Under this design, it is possible to have accounts that linger, never get touched, and never have to pay rent.  `Noop` instructions that name these accounts can be used to "garbage collect", but it'd also be possible for accounts that never get touched to migrate out of a validator's working set, thereby reducing memory consumption and obviating the need to charge rent.

### Ad-hoc collection

Collecting rent on an as-needed basis (i.e. whenever accounts were loaded/accessed) was considered. 
The issues with such an approach are:
* accounts loaded as "credit only" for a transaction could very reasonably be expected to have rent due, 
but would not be debitable during any such transaction
* a mechanism to "beat the bushes" (i.e. go find accounts that need to pay rent) is desirable, 
lest accounts that are loaded infrequently get a free ride

### System instruction for collecting rent

Collecting rent via a system instruction was considered, as it would naturally have distributed rent to active and stake-weighted nodes and could have been done incrementally. However:
* it would have adversely affected network throughput
* it would require special-casing by the runtime, as accounts with non-SystemProgram owners may be debited by this instruction
* someone would have to issue the transactions

### Account scans on every epoch

Scanning the entire Bank for accounts that owe rent at the beginning of each epoch was considered.  This would have been an expensive operation, and would require that the entire current state of the network be present on every validator at the beginning of each epoch.
