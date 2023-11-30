---
title: What is rent?
description: "Rent: A deprecated concept no longer used on the Solana block chain."
---

## Rent exempt

All accounts must maintain a minimum LAMPORT balance.  This is called the "rent-exempt balance" or "rent-exempt
minimum" even though "rent" is an outdated concept no longer applied to Solana accounts.  Quite simply, the
"rent-exempt minimum" balance of an account is the minimum number of lamports which must be held in the account, and
is determined by the number of bytes stored in the account, with larger accounts having larger "rent-exempt minimum"
balances.

The RPC endpoints have the ability to calculate this [estimated rent exempt balance](../../api/http#getminimumbalanceforrentexemption) and is recommended to be used.

Every time an account's balance is reduced, a check is performed to see if the account is still above the "rent-exempt minimum" balance. Transactions that would cause an account's balance to drop below this balance will fail.

## Learn more about the deprecated Rent feature

You can learn more about how Solana Rent was originally designed with the following articles and documentation.
Keep in mind that Rent is now deprecated and only aspects of this documentation referring to Rent-Exempt
Minimum balances still applies:

- [Implemented Proposals - Rent](../../implemented-proposals/rent.md)
