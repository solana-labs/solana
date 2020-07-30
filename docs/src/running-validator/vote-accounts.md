---
title: Vote Account Management
---

This page describes how to set up an on-chain _vote account_.   Creating a vote
account is needed if you plan to run a validator node on Solana.

All settings for a vote account can be configured at the time the account is
created, or they can be set after the account is created while the corresponding
validator is running.

### Vote Account Address

A vote account is created at an address that is either the public key of a
keypair file, or at a derived address based on a keypair file's public key and
a seed string.

The address of a vote account does not need to have a stored corresponding
private key, because the address (although it may be a valid public key) is never
needed to sign any transactions, but is just used to look up the account
information.

When someone wants to [delegate tokens in a stake account](../staking.md),
the delegation command is pointed at the vote account address of the validator
to whom the token-holder wants to delegate.

### Validator Identity

The _validator identity_ is a system account that is used to pay for all the
vote transaction fees submitted by the vote account.
Because the vote account is expected to vote on most valid blocks it receives,
the validator identity account is frequently
(potentially multiple times per second) signing transactions and
paying fees.  For this reason the validator identity account's keypair must be
stored as a "hot wallet" in a keypair file on the same system the validator
process is running.

Because a hot wallet is generally less secure than an offline or "cold" wallet,
we only recommend storing enough SOL on the identity account to cover voting fees
for some amount of time (perhaps a few weeks or months) and periodically topping
off your validator identity account from a more secure wallet.

The validator identity is required to be provided when a vote account is created.
The validator identity can also be changed after an account is created by using
the `vote-update-validator` command.

### Vote Authority

The _vote authority_ keypair is used to sign each vote transaction the validator
node wants to submit to the cluster.  This doesn't necessarily have to be unique
from the validator identity, as you will see later in this document.  Because
the vote authority, like the validator identity, is signing transactions
frequently, this also must be a hot keypair on the same file system as the
validator process.

It is recommended to set the vote authority to the same address as the validator
identity.  If the validator identity is also the vote authority, only one
signature per vote transaction is needed in order to both sign the vote and pay
the transaction fee.  Because transaction fees on Solana are assessed
per-signature, having one signer instead of two will result in half the transaction
fee paid compared to setting the vote authority and validator identity to two
different accounts.

The vote authority can be set when the vote account is created.  If it is not
provided, the default behavior is to assign it the same as the validator identity.
The vote authority can be changed later with the `vote-authorize-voter` command.

The vote authority can be changed at most once per epoch.  If the authority is
changed with `vote-authorize-voter`, this will not take effect until the
beginning of the next epoch.  To support a smooth transition of the vote signing,
`solana-validator` allows the `--authorized-voter` argument to be specified
multiple times.  This allows the validator process to keep voting successfully
when the network reaches an epoch boundary at with the validator's vote authority
account changes.

### Withdraw Authority

The _withdraw authority_ keypair is used to withdraw funds from a vote account
using the `withdraw-from-vote-account` command.  Any network rewards a validator
earns are deposited into the vote account and are only retrievable by signing
with the withdraw authority keypair.

The withdraw authority is also required to sign any transaction to change
a vote account's [commission](#commission), and to change the validator
identity on a vote account.

Because the vote account could accrue a significant balance, it is recommended
to keep the withdraw authority keypair in an offline/cold wallet, as it is
not needed to sign frequent transactions.

The withdraw authority can be set at vote account creation with the
`--authorized-withdrawer` option.  If this is not provided, the validator
identity will be set as the withdraw authority by default.

The withdraw authority can be changed later with the `vote-authorize-withdrawer`
command.

### Commission

_Commission_ is the percent of network rewards earned by a validator that are
deposited into the validator's vote account.  The remainder of the rewards
are distributed to all of the stake accounts delegated to that vote account,
proportional to the active stake weight of each stake account.

For example, if a vote account has a commission of 10%, for all rewards earned
by that validator in a given epoch, 10% of these rewards will be deposited into
the vote account in the first block of the following epoch. The remaining 90%
will be deposited into delegated stake accounts as immediately active stake.

Commission can be set upon vote account creation with the `--commission` option.
If it is not provided, it will default to 100%.

Commission can also be changed later with the `vote-update-commission` command.

When setting the commission, only integer values in the set [0-100] are accepted.
The integer represents the number of percentage points for the commission, so
creating an account with `--commission 10` will set a 10% commission.
