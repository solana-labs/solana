---
title: Stake Account Structure
---

A stake account on Solana can be used to delegate tokens to validators on
the network to potentially earn rewards for the owner of the stake account.
Stake accounts are created and managed differently than a traditional wallet
address, known as a _system account_. A system account is only able to send and
receive SOL from other accounts on the network, whereas a stake account supports
more complex operations needed to manage a delegation of tokens.

Stake accounts on Solana also work differently than those of other Proof-of-Stake
blockchain networks that you may be familiar with. This document describes the
high-level structure and functions of a Solana stake account.

#### Account Address

Each stake account has a unique address which can be used to look up the account
information in the command line or in any network explorer tools. However,
unlike a wallet address in which the holder of the address's keypair controls
the wallet, the keypair associated with a stake account address does not necessarily have
any control over the account. In fact, a keypair or private key may not even
exist for a stake account's address.

The only time a stake account's address has a keypair file is when [creating
a stake account using the command line tools](../cli/delegate-stake.md#create-a-stake-account).
A new keypair file is created first only to ensure that the stake account's
address is new and unique.

#### Understanding Account Authorities

Certain types of accounts may have one or more _signing authorities_
associated with a given account. An account authority is used to sign certain
transactions for the account it controls. This is different from
some other blockchain networks where the holder of the keypair associated with
the account's address controls all of the account's activity.

Each stake account has two signing authorities specified by their respective address,
each of which is authorized to perform certain operations on the stake account.

The _stake authority_ is used to sign transactions for the following operations:

- Delegating stake
- Deactivating the stake delegation
- Splitting the stake account, creating a new stake account with a portion of the
  funds in the first account
- Merging two stake accounts into one
- Setting a new stake authority

The _withdraw authority_ signs transactions for the following:

- Withdrawing un-delegated stake into a wallet address
- Setting a new withdraw authority
- Setting a new stake authority

The stake authority and withdraw authority are set when the stake account is
created, and they can be changed to authorize a new signing address at any time.
The stake and withdraw authority can be the same address or two different
addresses.

The withdraw authority keypair holds more control over the account as it is
needed to liquidate the tokens in the stake account, and can be used to reset
the stake authority if the stake authority keypair becomes lost or compromised.

Securing the withdraw authority against loss or theft is of utmost importance
when managing a stake account.

#### Multiple Delegations

Each stake account may only be used to delegate to one validator at a time.
All of the tokens in the account are either delegated or un-delegated, or in the
process of becoming delegated or un-delegated. To delegate a fraction of your
tokens to a validator, or to delegate to multiple validators, you must create
multiple stake accounts.

This can be accomplished by creating multiple stake accounts from a wallet
address containing some tokens, or by creating a single large stake account
and using the stake authority to split the account into multiple accounts
with token balances of your choosing.

The same stake and withdraw authorities can be assigned to multiple
stake accounts.

#### Merging stake accounts

Two stake accounts that have the same authorities and lockup can be merged into
a single resulting stake account. A merge is possible between two stakes in the
following states with no additional conditions:

- two deactivated stakes
- an inactive stake into an activating stake during its activation epoch

For the following cases, the voter pubkey and vote credits observed must match:

- two activated stakes
- two activating accounts that share an activation epoch, during the activation epoch

All other combinations of stake states will fail to merge, including all "transient"
states, where a stake is activating or deactivating with a non-zero effective stake.

#### Delegation Warmup and Cooldown

When a stake account is delegated, or a delegation is deactivated, the operation
does not take effect immediately.

A delegation or deactivation takes several [epochs](../terminology.md#epoch)
to complete, with a fraction of the delegation becoming active or inactive at
each epoch boundary after the transaction containing the instructions has been
submitted to the cluster.

There is also a limit on how much total stake can become delegated or
deactivated in a single epoch, to prevent large sudden changes in stake across
the network as a whole. Since warmup and cooldown are dependent on the behavior
of other network participants, their exact duration is difficult to predict.
Details on the warmup and cooldown timing can be found
[here](../cluster/stake-delegation-and-rewards.md#stake-warmup-cooldown-withdrawal).

#### Lockups

Stake accounts can have a lockup which prevents the tokens they hold from being
withdrawn before a particular date or epoch has been reached. While locked up,
the stake account can still be delegated, un-delegated, or split, and its stake
authority can be changed as normal. Only withdrawal into another wallet or
updating the withdraw authority is not allowed.

A lockup can only be added when a stake account is first created, but it can be
modified later, by the _lockup authority_ or _custodian_, the address of which
is also set when the account is created.

#### Destroying a Stake Account

Like other types of accounts on the Solana network, a stake account that has a
balance of 0 SOL is no longer tracked. If a stake account is not delegated
and all of the tokens it contains are withdrawn to a wallet address, the account
at that address is effectively destroyed, and will need to be manually
re-created for the address to be used again.

#### Viewing Stake Accounts

Stake account details can be viewed on the [Solana Explorer](http://explorer.solana.com/accounts)
by copying and pasting an account address into the search bar.
