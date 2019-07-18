# Rent

Accounts on Solana may have owner-controlled state (`Account::data`) that's separate 
from the account's balance (`Account::lamports`).  Since validators on the network need 
to maintain a working copy of this state in memory, the network charges a time-and-space 
based fee for this resource consumption, also known as Rent.

## Two-tiered rent regime

Accounts which maintain a minimum balance equivalent to 2 years of rent are exempt from
rent.  Accounts whose balance falls below this threshold are charged rent 
at a rate specified in genesis, in lamports per kilobyte per year.

## Collecting rent

A field on each account (`Account::rent_due_slot`) keeps track of the next slot at which the account will 
owe rent, similar to a time-to-live.

Rent is charged directly from the account balance via issuance of `SystemInstruction::RentDue` and is deposited into 
the current leader's account as transaction fees.  `SystemInstruction::RentDue` charges rent according to the genesis rent rate for:

1. any slots in arrears of the current slot
2. one epoch worth of slots into the future

Assumming sufficient funds are present, `SystemInstruction::RentDue` sets `Account::rent_due_slot` to a value that is `slots_per_epoch` (for the current epoch) ahead of the current slot.

In this way, rent is charged and distributed in a stake-weighted round-robin fashion among the network's validators.

## Initialization

Initial `Account::rent_due_at_lot` is initialized by `SystemInstruction::CreateAccount`.  Rent is paid to the leader as in `RentDue`.

## Deliquency

Accounts whose balance reaches zero due to rent collection have their `Account::data` dropped and replaced with a hash of the contents.

## Issuing `SystemInstruction::RentDue`

Any node may issue `SystemInstruction::RentDue` and pay the required transaction fee, but the beneficiary is always the slot leader.

## Design considerations

### Ad-hoc collection

Collecting rent on an as-needed basis (i.e. whenever accounts were loaded/accessed) was considered. 
The issues with such an approach are:
* accounts loaded as "credit only" for a transaction could very reasonably be expected to have rent due, 
but would not be debitable during any such  transaction
* a mechanism to "beat the bushes" (i.e. go find accounts that need to pay rent) is desirable, 
lest accounts that are loaded infrequently get somewhat of a free ride.

### Collect from everybody instead?

Use of a transcation to collect rent could have been avoided with a (likely costly) per-slot or per-epoch scan of all current 
balances.

### Warts

The introduction of `SystemInstruction::RentDue` requires special-casing by the runtime, as accounts with non-SystemProgram
owners may be debited by this instruction.
