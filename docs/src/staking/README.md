# Staking on Solana

## What is Staking?
Some high level info about PoS networks and staking.

https://lunie.io/guides/what-is-staking/
https://academy.binance.com/blockchain/what-is-staking

## Why should I stake my SOL tokens?
 - Help secure the network
 - Grow with inflation, once enabled by Solana Foundation
 - [Earn rewards](../implemented-proposals/staking-rewards.md)

## How do I stake?
#### Create a Stake Account
A stake account is a different type of account from a wallet address
that is used to simply send and receive SOL tokens to other addresses. If you
have received SOL in a wallet address you control, you can move some of
these tokens into a new stake account, which will have a different address
than the wallet you used to create it.  Depending on which wallet you are using
the steps to create a stake account may vary slightly.  Not all wallets support
staking operations, see [Supported Wallets](#supported-wallets).

If you have been transferred control of an existing stake account and wish to
use it, you do not need to create a new stake account.

#### Select a Validator
After a stake account is created, you will likely want to delegate the account
to a validator node.  Below are a few places where you can get information about
the validators who are currently participating in running the network.
Solana does not recommend any particular validator or validators.

On this Solana Forum thread, our validators introduce themselves, along with
some description of their history and services:
 - https://forums.solana.com/t/validator-information-thread

The site solanabeach.io is built and maintained by one of our validators,
Staking Facilities.  It provides a some high-level graphical information about
the network as a whole, as well as a list of each validator and some recent
performance statistics about each one.
 - https://solanabeach.io

Using the Solana command line tools, the following commands will display the
complete validator list to the terminal, along with currently delegated stake
and recent block production performance statistics, respectively.
 - `solana validators`
 - `solana block-production`

#### Delegate your Stake
Once you have decided to which validator or validators you will delegate, use
a supported wallet to delegate your stake account to the validator's vote
account address.

## Supported Wallets
Currently, staking operation are only supported by wallets that can interact
with the Solana command line tools, including Ledger Nano S and paper wallet.

We are working hard to integrate with additional wallets that will provide a
friendlier user interface for people who are not familiar with using command
line tools, please stay tuned for more information!

[Staking commands using the Solana Command Line Tools](../cli/delegate-stake.md)

## Stake Account Details
For more information about the operations and permissions associated with a
stake account, please see [Stake Accounts](stake-accounts.md)
