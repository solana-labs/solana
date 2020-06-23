*Note before reading: All references to increases in values are in absolute
terms with regards to balance of SOL.
This document makes no suggestion as to the monetary value of SOL at any time.*

# Staking on Solana

Staking your SOL tokens on Solana is the best way you can help secure the world's
highest-performing blockchain network, and
[earn rewards](../implemented-proposals/staking-rewards.md) for doing so!
Inflation and network rewards are *NOT* presently implemented on Solana's
Mainnet Beta network, but may be enabled in the future.

Solana is a Proof-of-Stake (PoS) network with delegations, which means that
anyone who holds SOL tokens can choose to delegate some of their SOL to one or
more validators, who process transactions and run the network.

Delegating stake is a shared-risk shared-reward financial model that may provide
returns to holders of tokens delegated for a long period.
This is achieved by aligning the financial incentives of the token-holders
(delegators) and the validators to whom they delegate.

The more stake a validator has delegated to them, the more often this validator
is chosen to write new transactions to the ledger.  The more transactions
the validator writes, the more rewards they and their delegators earn.
Validators who configure their systems to be able to process more transactions
at a time not only earn proportionally more rewards for doing so, they also
keep the network running as fast and as smoothly as possible.

Validators incur costs by running and maintaining their systems, and this is
passed on to delegators in the form of a fee collected as a percentage of
rewards earned.  This fee is known as a *commission*. As validators earn more
rewards the more stake is delegated to them, they may compete with one another
to offer the lowest commission for their services, in order to attract more
delegated stake.

There is a risk of loss of tokens when staking, through a process known as
*slashing*.  Slashing is *NOT* presently enabled on Solana's Mainnet Beta
network, but may be implemented in the future.  Slashing involves the automatic
removal and destruction of a portion of a validator's delegated stake in
response to intentional malicious behavior, such as creating invalid
transactions or censoring certain types of transactions or network participants.
If a validator is slashed, all token holders who have delegated stake to that
validator will lose a portion of their delegation.  While this means an immediate
loss for the token holder, it also is a loss of future rewards for the validator
due to their reduced total delegation.

It is the goal of the network rewards and slashing to align both validators'
and token holders' financial incentives, which in turn help keeps the network
secure, robust and performing at its best.

*Note: Network rewards for stakers and validators are not presently enabled on
Mainnet Beta.*

*Note: Slashing is not implemented on Mainnet Beta at this time.*


## How do I stake my SOL tokens?
In order to stake tokens on Solana, you first will need to transfer some SOL
into a wallet that supports staking, then follow the steps or instructions
provided by the wallet to create a stake account and delegate your stake.
Different wallets will vary slightly in their process for this but the general
description is below.

#### Supported Wallets
Currently, staking operation are only supported by wallets that can interact
with the Solana command line tools, including Ledger Nano S and paper wallet.

[Staking commands using the Solana Command Line Tools](../cli/delegate-stake.md)

#### Create a Stake Account
A stake account is a different type of account from a wallet address
that is used to simply send and receive SOL tokens to other addresses. If you
have received SOL in a wallet address you control, you can use some of
these tokens to create and fund a new stake account, which will have a different
address than the wallet you used to create it.
Depending on which wallet you are using the steps to create a stake account
may vary slightly.  Not all wallets support stake accounts, see
[Supported Wallets](#supported-wallets).

#### Select a Validator
After a stake account is created, you will likely want to delegate the SOL
to a validator node.  Below are a few places where you can get information about
the validators who are currently participating in running the network.
The Solana Labs team and the Solana Foundation do not recommend any particular
validator.

The Mainnet Beta validators introduce themselves and their services on this
Solana Forum thread:
 - https://forums.solana.com/t/validator-information-thread

The site solanabeach.io is built and maintained by one of our validators,
Staking Facilities.  It provides a some high-level graphical information about
the network as a whole, as well as a list of each validator and some recent
performance statistics about each one.
 - https://solanabeach.io

To view block production statistics, use the Solana command-line tools:
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
