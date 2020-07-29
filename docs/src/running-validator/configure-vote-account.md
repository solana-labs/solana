---
title: Configuring a Vote Account
---

This page describes how to set up an on-chain vote account.   Creating a vote
account is needed if you plan to run a validator node on Solana.

All settings for a vote account can be configured at the time the account is
created, or they can be set after the account is created while the corresponding
validator is running.

## Vote Account Structure

### Vote Account Address

A vote account is created at an address that is either the public key of a
keypair file, or at a derived address from the authorized voter's public key.

The address of a vote account does not need to have a stored corresponding
private key, because the address (although it may be a valid public key) is never
needed to sign any transactions, but is just used to look up the account
information.

When someone wants to [delegate tokens in a stake account](../staking.md),
the delegation command is pointed at the vote account address of the validator
to whom the token-holder wants to delegate.

### Validator Identity

The Validator Identity is a system account that pays for all the vote transaction
fees submitted by the vote account.  Because the vote account is expected to vote
on most valid blocks it receives, the Validator Identity account is frequently
(as often as once per slot, or roughly every 400ms) signing transactions and
paying fees.  For this reason the Validator Identity account must be stored as
a "hot wallet" as a keypair file on the same system the validator process is
running.

Because a hot wallet is generally less secure than an offline or "cold" wallet,
we only recommend storing enough SOL on the Identity keypair to cover voting fees
for some amount of time (perhaps a few weeks or months) and periodically topping
off your Validator Identity account from a more secure wallet.

The Validator Identity is required to be provided when a vote account is created.
The Validator Identity can also be changed after an account is created by using
the `vote-update-validator` command.

### Authorized Voter

The Authorized Voter keypair is used to sign each vote transaction the validator
node wants to submit to the cluster.  This can be the same keypair used as the
Validator Identity for the same vote account, or a different keypair.  Because
the Authorized Voter, like the Validator Identity, is signing transactions
frequently, this also must be a hot keypair on the same file system as the
validator process.

It is recommended to set the Authorized Voter to the same address as the Validator
Identity.  If the Validator Identity is also the Authorized Voter, only one
signature per vote transaction is needed in order to both sign the vote and pay
the transaction fee.  Because transaction fees on Solana are assessed
per-signature, having one signer instead of two will result in half the transaction
fee paid compared to setting the Voter and Identity to two different accounts.

The Authorized Voter can be set when the vote account is created.  If it is not
provided, the vote account with set the Validator Identity as the Authorized
Voter by default.  The Authorized Voter can be changed later with the
`vote-authorize-voter` command.

The Authorized Voter can be changed at most once per epoch.  If the Voter is
changed with `vote-authorize-voter`, this will not take effect until the end of
the current epoch.  To support a smooth transition of the vote signing,
`solana-validator` allows the `--authorized-voter` argument to be specified
multiple times.  This allows the validator process to keep voting successfully
when the network reaches an epoch boundary at with the validator's authorized
voter account changes.

### Authorized Withdrawer

### Commission

## Vote Account Operations
```bash
    create-vote-account            Create a vote account
    vote-account                   Show the contents of a vote account
    vote-authorize-voter           Authorize a new vote signing keypair for the given vote account
    vote-authorize-withdrawer      Authorize a new withdraw signing keypair for the given vote account
    vote-update-commission         Update the vote account's commission
    vote-update-validator          Update the vote account's validator identity
    withdraw-from-vote-account     Withdraw lamports from a vote account into a specified account
```

## Credits

## Commission/Inflation/Delegation