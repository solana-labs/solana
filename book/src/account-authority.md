# Account Authority #

## Problem ##

There's no extant general facility to re-key or to delegate authority of an account in Solana. This manifests as a problem in several ways:

* programs that wish to support re-keying or alternative authority must implement this on their own, leading to replicode and 
(replicated) bugs
* programs that require authority that don't implement it themselves (i.e. use the account's address as the authority) expose 
themselves to grinding attacks or require some kind of manual re-keying, account copying
* programs that:
   1) require authority of the account and
   2) don't implement alternative authority
require that authorized transactions carry multiple signatures (one for issuance of the instruction and one for the account's authority),
making them inaccessible to wallets that can only handle one keypair
* programs that carry pubkeys as addresses in state expose their targets to grinding attacks, force those addressed accounts to 
implement alt-authority, or require big, costly re-keying efforts (e.g. multiple stakes pointing at a vote account, if the vote account
is re-keyed, *all* delegating stakes must be updated)

## Proposed Solution ##
Add an `authority` field to `Account` that defaults to `None`, `Pukey::default()` (or the Account's address) that can be updated 
via a system instruction.  This field would be available to programs for simple authority checks during instruction processing, 
and would be protected from modification by the runtime (only the system program may modify).
