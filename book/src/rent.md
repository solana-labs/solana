# Rent

Accounts on Solana may have owner-controlled state (`Account::data`) that's separate 
from the account's balance (`Account::lamports`).  As validators on the network need 
to maintain a working copy of this state in memory, the network charges a time-and-space 
based fee for the memory consumption, also known as Rent.

## Two-tiered rent regime

Accounts which maintain a minimum balance equivalent to 2 years of rent are exempt from
rent.  Accounts whose lamport balance falls below this threshold are charged rent 
at a rate specified in genesis, in lamports per kilobyte per year.

## Collecting rent

A field on each account (`Account::rent_due_slot`) keeps track of the next slot at which the account will 
next owe rent, similar to a time-to-live.

Rent is charged directly from the account balance via issuance of `SystemInstruction::RentDue` and is deposited into 
the current leader's account as transaction fees.  `SystemInstruction::RentDue` charges one epoch worth of 
rent according to the rate specified in `RentCalculator` and increments `Account::rent_due_slot` by 
`slots_per_epoch` for the current epoch.

In this way, rent is charged and distributed in a stake-weighted round-robin fashion among the validators.

## Initialization

Initial `Account::rent_due_at_lot` is initialized by `SystemInstruction::CreateAccount`.  Rent is paid to the leader as in `RentDue`.

## Deliquency

Accounts that run out of lamport balance due to rent collection have their `Account::data` dropped and replaced with a hash those bytes.

## Issuing `SystemInstruction::RentDue`

Any node may issue `SystemInstruction::RentDue` and pay the required transaction fee, but the beneficiary is always the slot leader.
