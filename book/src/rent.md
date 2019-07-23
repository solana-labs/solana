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

The first bank in each new epoch scans all accounts for any that would owe rent for the previous epoch.

Rent is deducted from subject accounts according to the rate specified in genesis.

A percentage of the rent collected is destroyed, the rest is distributed to validator accounts by stake weight, a la transaction fees.  Any fractional lamports are destroyed.

## Deliquency

Accounts whose balance reaches zero due to rent collection are dropped.

## Design considerations, others considered

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

