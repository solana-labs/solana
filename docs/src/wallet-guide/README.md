# Solana Wallet Guide
This document describes the different wallet options that are available to users
of Solana who want to be able to send, receive and interact with
SOL tokens on the Solana blockchain.

## What is a Wallet?
A crypto wallet is a device or application that stores a collection of keys and
can be used to send, receive,
and track ownership of cryptocurrencies.  Wallets can take many forms.
A wallet might be a directory or file in your computer's file system,
a piece of paper, or a specialized device called a *hardware wallet*.
There are also various smartphone apps and computer programs
that provide a user-friendly way to create and manage wallets.

A *keypair* is a securely generated *private key* and its
cryptographically-derived *public key*.  A private key and its corresponding
public key are together known as a *keypair*.
A wallet contains a collection of one or more keypairs and provides some means
to interact with them.

The *public key* (commonly shortened to *pubkey*) is known as the wallet's
*receiving address* or simply its *address*.  The wallet address **may be shared
and displayed freely**.  When another party is going to send some amount of
cryptocurrency to a wallet, they need to know the wallet's receiving address.
Depending on a blockchain's implementation, the address can also be used to view
certain information about a wallet, such as viewing the balance,
but has no ability to change anything about the wallet or withdraw any tokens.

The *private key* is required to digitally sign any transactions to send
cryptocurrencies to another address or to make any changes to the wallet.
The private key **must never be shared**.  If someone gains access to the
private key to a wallet, they can withdraw all the tokens it contains.
If the private key for a wallet is lost, any tokens that have been sent
to that wallet's address are **permanently lost**.

Different wallet solutions offer different approaches to keypair security and
interacting with the keypair and sign transactions to use/spend the tokens.
Some are easier to use than others.
Some store and back up private keys more securely.
Solana supports multiple types of wallets so you can choose the right balance
of security and convenience.

**If you want to be able to receive SOL tokens on the Solana blockchain,
you first will need to create a wallet.**

## Supported Wallets
Solana supports supports several types of wallets in the Solana native
command-line app as well as wallets from third-parties.

For the majority of users, we recommend using one of the
[app wallets](apps.md), which will provide a more familiar user
experience rather than needing to learn command line tools.

{% page-ref page="apps.md" %}

For advanced users or developers, the [command-line wallets](cli.md)
may be more appropriate, as new features on the Solana blockchain will always be
supported on the command line first before being integrated into third-party
solutions.

{% page-ref page="cli.md" %}