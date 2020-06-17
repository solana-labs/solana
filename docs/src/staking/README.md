# Staking on Solana

After you have [received SOL](transfer-tokens.md), you might consider putting
it to use by delegating *stake* to a validator. Stake is what we call tokens
in a *stake account*. Solana weights validator votes by the amount of stake
delegated to them, which gives those validators more influence in determining
then next valid block of transactions in the blockchain. Solana then generates
new SOL periodically to reward stakers and validators. Holders of
staked tokens will [earn rewards](../implemented-proposals/staking-rewards.md)
proportional to how many tokens they have staked.

*Note: Network rewards for stakers and validators are not presently enabled.
It is the decision of the Solana Foundation if/when to enable such rewards.*

Staking helps secure the Solana network. The more tokens that are staked makes
the network less susceptible to censorship and certain kinds of attacks.

## How do I stake my SOL tokens?
In order to stake tokens on Solana, you first will need to transfer some SOL
into a wallet that supports staking, then follow the steps or instructions
provided by the wallet to create a stake account and delegate your stake.
Different wallets will vary slightly in their process for this but the general
description is below.

#### Supported Wallets
Currently, staking operation are only supported by wallets that can interact
with the Solana command line tools, including Ledger Nano S and paper wallet.

We are working hard to integrate with additional wallets that will provide a
friendlier user interface for people who are not familiar with using command
line tools, please stay tuned for more information!

[Staking commands using the Solana Command Line Tools](../cli/staking-operations.md)

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
The Solana team does not recommend any particular validator.

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

The Solana team does not make recommendations on how to interpret this
information.  Potential delegators should do their own due diligence.

#### Delegate your Stake
Once you have decided to which validator or validators you will delegate, use
a supported wallet to delegate your stake account to the validator's vote
account address.

## Stake Account Details
For more information about the operations and permissions associated with a
stake account, please see [Stake Accounts](stake-accounts.md)
