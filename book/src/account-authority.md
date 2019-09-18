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


### Authority vs. Address ###
Solana currently conflows account _address_, (i.e. its name, where it lives in the bank) with its _authority_ (i.e. who needs to 
sign transactions that modify the state of the account).  Under this proposal, these concepts would be separate, or separable.  
Changing an account's authority would require the signature of the current authority (aka _re-keying_).

### Re-keying today ###
Account rekeying is currently unimplemented in Solana, but some programs partially implement this by holding the notion of 
authority or partial authority in account state.  Examples:
* Vote program: vote program holds an `authorized_voter` initialized to the account's address, but updatable with an transaction 
signed by the current `authorized_voter`.  This allows for rekeying of the voting keypair, but does not allow rekeying of the vote account itself.  The vote account requires the signature of the account address keypair to effect withdrawal
* Stake program: the stake program holds an `authority` pubkey, initialized to the account's address and updatable by the current `authority`.  The intended use is to allow system programs (those that can issue transactions) to be able to do authorization-required operations like delegate, deactivate, and withdraw.
* Config program: contains a list of pubkeys authorized to change account state (including update the authorized set).


## Proposed Solution ##
Add an `authority` field to `Account` that defaults to `None`, `Pukey::default()` (or the Account's address) that can be updated 
via a system instruction.  This field would be available to programs for simple authority checks during instruction processing, 
and would be protected from modification by the runtime (only the system program may modify).


