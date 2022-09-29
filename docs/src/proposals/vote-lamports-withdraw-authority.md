---
title: Vote Accounts Lamports Authority
---

## Vote Accounts Lamports Authority

This design describes a modification to add a new authority type to vote
accounts, which will allow withdrawing lamports from vote accounts and no
other operation on the account.

The fundamental problem that is being solved here is that the current vote
account authority structure requires withdrawing lamports from the vote
account using the vote accounts "master" authority.  The withdraw authority of
a vote account is the master authority in that it is authorized to perform all
operations on the vote account, including:

- Set authorized voter
- Set commission
- Withdraw lamports
- Set new withdraw authority

This makes the withdraw authority an incredibly security sensitive key -- if
it is stolen, it can be used to completely steal the vote account, setting a
new withdraw authority no longer under control of the validator operator.

Requiring validator operators to use this highly sensitive key every time they
want to withdraw lamports from the vote account is very undesirable.


### Proposed Change

Add a new authority type to vote accounts.  Its purpose would be to allow
withdrawing lamports from the vote account, and it would not have
authorization to perform any other operation on vote accounts.

Unfortunately the name "withdraw authority" is already used for vote accounts;
it's a misnomer because it has much more authority than just withdrawing.  But
since the name is already well established in the validator community, a new
name is proposed for this new authority rather than trying to re-use an
existing name.

The proposed name is "lamports authority".  This hopefully makes it very clear
that this authority has no purpose other than to allow withdrawing lamports
from the vote account.


### Method of Implementation

The vote program would be modified to utilize a new Program Derived Account
type: the "lamports authority config account".  The address of these accounts
would be derived as the PDA of { "lamports authority", <VOTE_ACCOUNT_ADDRESS>
}.

The contents of this account would be simply the 32 bytes of the pubkey of the
lamports authority.

The VoteAuthorize enum will be updated to include a new entry: Lamports.

The implementation of VoteState.authorize() will be updated such that when a
new lamports authority is to be authorized, it will compute the PDA to be used
to store it, check that the vote account withdraw authority is a signer of the
transaction, and if so, write the new lamports authority into that PDA.  Note
that the PDA must be included in the instruction's account list as a writable
account, which means that the Authorize instruction will have a new optional
account reference: "3. `[WRITE]` lamports authority config account
(optional)".

The AuthorizeChecked and AuthorizeWithSeed instruction handling will similarly
be updated as necessary.

Additionally the Withdraw instruction processing will be updated.  The third
account reference will be changed to: "2. `[SIGNER]` Withdraw authority or
lamports authority" and a new optional third account reference will be added:
"3. `[]` Lamports authority config account (optional)".  If this fourth
optional account is present, then processing the withdraw instruction will
mean:

1. Computing the PDA of the lamports authority config account
2. Checking that this PDA is as provided in address (3)
3. Checking that this PDA is writable
4. Checking that this PDA is owned by the Vote program
5. Reading the contents of the PDA and checking that they match the lamports
   authority provided in account reference (2)
6. Ensuring that the account reference in (2) is a signer of the transaction

If 1 - 6 succeed, then the withdraw continues as normal.


### Additional Changes Needed

The solana command line utility would need to be updated to provide a few new
commands:

1. vote-authorize-lamports-authority
2. vote-authorize-lamports-authority-checked

Also the withdraw-from-vote-account command would need a new option:

--lamports-authority <AUTHORIZED_KEYPAIR>

This would specify a lamports authority to use which would then cause the
command line to construct a lamports authority based withdraw transaction.

In addition, the vote-account command should look up the lamports authority
and present it in the command output if it is present for the queried vote
account.
