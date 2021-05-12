---
title: SolFlare Web Wallet
---

## Introduction

[SolFlare.com](https://solflare.com/) is a community-created web wallet built
specifically for Solana.
SolFlare supports sending and receiving native SOL tokens as well as sending and
receiving SPL Tokens (Solana's ERC-20 equivalent).
SolFlare also supports staking of SOL tokens.

As a _non-custodial_ wallet, your private keys are not stored by the SolFlare
site itself, but rather they are stored in an encrypted
[Keystore File](#using-a-keystore-file) or on a
[Ledger Nano S or X hardware wallet](#using-a-ledger-nano-hardware-wallet).

This guide describes how to set up a wallet using SolFlare, how to send and
receive SOL tokens, and how to create and manage a stake account.

## Getting Started

Go to https://www.solflare.com in a supported browser. Most popular web browsers
should work when interacting with a Keystore File, but currently only
Chrome and Brave are supported when interacting with a Ledger Nano.

### Using a Keystore File

#### Create a new Keystore File

To create a wallet with a Keystore file, click on "Create a Wallet" and select
"Using Keystore File". Follow the prompts to create a password which will be
used to encrypt your Keystore file, and then to download the new file to your
computer. You will be prompted to then upload the Keystore file back to the site
to verify that the download was saved correctly.

**NOTE: If you lose your Keystore file or the password used to encrypt it, any
funds in that wallet will be lost permanently. Neither the Solana team nor the
SolFlare developers can help you recover lost keys.**

You may want to consider saving a backup copy of your Keystore file on an
external drive separate from your main computer, and storing your password in a
separate location.

#### Access your wallet with a Keystore File

To use SolFlare with a previously created Keystore file, click on
"Access a Wallet" and select "Using Keystore File". If you just created a new
Keystore file, you will be taken to the Access page directly.
You will be prompted to enter the password and upload your Keystore file,
then you will be taken to the wallet interface main page.

### Using a Ledger Nano hardware wallet

_NOTE: Please see [known issues](ledger-live.md#known-issues) for any current
limitations in using the Nano._

#### Initial Device Setup

To use a Ledger Nano with SolFlare, first ensure you have
[set up your Nano](ledger-live.md) and have [installed the latest version of
the Solana app](ledger-live.md#upgrade-to-the-latest-version-of-the-solana-app)
on your device.

#### Select a Ledger address to access

Plug in your Nano and open the Solana app so the device screen displays
"Application is Ready".

From the SolFlare home page, click "Access a Wallet" then select "Using Ledger
Nano S | Ledger Nano X". Under "Select derivation path", select the only option:

`` Solana - 44`/501`/ ``

Note: Your browser may prompt you to ask if SolFlare may communicate with your
Ledger device. Click to allow this.

Select an address to interact with from the lower drop down box then click "Access".

The Ledger device can derive a large number of private keys and associated
public addresses. This allows you to manage and interact with an arbitrary
number of different accounts from the same device.

If you deposit funds to an address derived from your Ledger device,
make sure to access the same address when using SolFlare to be able to access
those funds. If you connect to the incorrect address,
simply click Logout and re-connect with the correct address.

## Select a Network

Solana maintains [three distinct networks](../clusters), each of which has
its own purpose in supporting the Solana ecosystem. Mainnet Beta is selected by
default on SolFlare, as this is the permanent network where exchanges and other
production apps are deployed. To select a different network, click on the name
of the currently selected network at the top of the wallet dashboard, either
Mainnet, Testnet or Devnet, then click on the name of the network you wish to be
using.

## Sending and Receiving SOL Tokens

### Receiving

To receive tokens into your wallet, someone must transfer some to your wallet's
address. The address is displayed at the top-left on the screen, and you can
click the Copy icon to copy the address and provide it to whoever is sending you
tokens. If you hold tokens in a different wallet or on an exchange, you can
withdraw to this address as well. Once the transfer is made, the balance shown
on SolFlare should update within a few seconds.

### Sending

Once you have some tokens at your wallet address, you can send them to any other
wallet address or an exchange deposit address by clicking "Transfer SOL" in the
upper-right corner. Enter the recipient address and the amount of SOL to
transfer and click "Submit". You will be prompted to confirm the details of the
transaction before you [use your key to sign the transaction](#signing-a-transaction)
and then it will be submitted to the network.

## Staking SOL Tokens

SolFlare supports creating and managing stake accounts and delegations. To learn
about how staking on Solana works in general, check out our
[Staking Guide](../staking).

### Create a Stake Account

You can use some of the SOL tokens in your wallet to create a new stake account.
From the wallet main page click "Staking" at the top of the page. In the upper-
right, click "Create Account". Enter the amount of SOL you want to use to
fund your new stake account. This amount will be withdrawn from your wallet
and transfered to the stake account. Do not transfer your entire wallet balance
to a stake account, as the wallet is still used to pay any transaction fees
associated with your stake account. Consider leaving at least 1 SOL in your
wallet account.

After you submit and [sign the transaction](#signing-a-transaction) you will see
your new stake account appear in the box labeled "Your Staking Accounts".

Stake accounts created on SolFlare set your wallet address as the
[staking and withdrawing authority](../staking/stake-accounts#understanding-account-authorities)
for your new account, which gives your wallet's key the authority to sign
for any transactions related to the new stake account.

### View your Stake Accounts

On the main Wallet dashboard page or on the Staking dashboard page, your stake
accounts will be visible in the "Your Staking Accounts" box. Stake accounts
exist at a different address from your wallet.

SolFlare will locate any display all stake accounts on the
[selected network](#select-a-network)
for which your wallet address is assigned as the
[stake authority](../staking/stake-accounts#understanding-account-authorities).
Stake accounts that were created outside of SolFlare will also be displayed and
can be managed as long as the wallet you logged in with is assigned as the stake
authority.

### Delegate tokens in a Stake Account

Once you have [selected a validator](../staking#select-a-validator), you may
delegate the tokens in one of your stake accounts to them. From the Staking
dashboard, click "Delegate" at the right side of a displayed stake account.
Select the validator you wish to delegate to from the drop down list and click
Delegate.

To un-delegate your staked tokens (also called deactivating your stake), the
process is similar. On the Staking page, at the right side of a delegated stake
account, click the "Undelegate" button and follow the prompts.

### Split a Stake Account

You may split an existing stake account into two stake accounts. Click on the
address of a stake account controlled by your wallet, and under the Actions bar,
click "Split". Specify the amount of SOL tokens you want to split. This will be
the amount of tokens in your new stake account and your existing stake account
balance will be reduced by the same amount. Splitting your stake account
allows you to delegate to multiple different validators with different amounts
of tokens. You may split a stake account as many times as you want, to create
as many stake accounts as you want.

## Signing a Transaction

Any time you submit a transaction such as sending tokens to another wallet or
delegating stake, you need to use your private key to sign the transaction so
it will be accepted by the network.

### Using a Keystore File

If you accessed your wallet using a Keystore file, you will be prompted to enter
your password any time the key is needed to sign a transaction.

### Using a Ledger Nano

If you accessed your wallet with a Ledger Nano, you will be prompted to
confirm the pending transaction details on your device whenever the key is needed
to sign. On the Nano, use the left and right buttons to view and confirm all of the
transaction details. If everything looks correct, keep clicking the right button
until the screen shows "Approve". Click both buttons to approve the transaction.
If something looks incorrect, press the right button once more so the screen shows
"Reject" and press both buttons to reject the transaction. After you approve
or reject a transaction, you will see this reflected on the SolFlare page.
