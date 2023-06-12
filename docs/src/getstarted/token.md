---
title: "Create a token"
description:
  "This quickstart guide will show you how to use the spl-token CLI to create a
  token."
keywords:
  - create token on solana
  - make coin on solana
  - set up spl-token cli
  - spl-token tutorial
  - intro to solana development
  - blockchain developer
  - blockchain tutorial
  - web3 developer
---

This quickstart guide will show you how to use the `spl-token` CLI to create a
token on the devnet, mint some tokens, and transfer them to another address.

## Prerequisites

This guide assumes you've completed [local development setup](./local.md) and
have a filesystem wallet created.

You'll also need to have `cargo` installed. The recommended way to install it is
with [rustup](https://www.rust-lang.org/tools/install). You can check if you
have it installed by running `cargo --version` in a terminal window.

### Configure the CLI to use devnet

We'll be using the devnet for this guide. Set the Solana CLI to use the devnet
by running:

```bash
solana config set --url devnet
```

Next, run `solana airdrop 2` to get some SOL for the devnet. You only need about
0.5 SOL to create a token, check your balance by running `solana balance`.

## The Solana Program Library

Tokens on Solana are created using the
[Token Program](https://spl.solana.com/token), which is part of the Solana
Program Library - a set of programs maintained by Solana Labs.

### How tokens are made

To create a fungible token on Solana, you interact with the SPL token program to
create a mint account. The mint account contains the token's metadata, including
the token's supply and decimals. Think of it like a factory that can mint new
tokens.

Users' wallets can't directly own tokens on Solana. For each token, a user has a
token account. The token account stores the balance of a token and is linked to
the user's wallet.

So to create a token and add them into your wallet, you need to:

1. Create a token mint account
2. Create a token account for your wallet
3. Mint tokens into your token account

Here's a simplified visual representation of this relationship:
![Token mint and token account relationship](/img/wallet-token-mint.svg)

### Install the spl-token-cli

The SPL has a CLI tool and a Javascript SDK that lets you interact with the
on-chain programs. We'll use the CLI tool. Install it by running:

```bash
cargo install spl-token-cli
```

This may take a few minutes to run.

### Create a fungible token

To create fungible token, you'll first need to create a mint account. Run this
command to interact with the token program:

```bash
spl-token create-token
```

This will give you:

```bash
Creating token FpFppjxbnSwX7kBX9X1K5FZLG1N4qnJxAxj1D7VB7gk9 under program TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA

Address:  FpFppjxbnSwX7kBX9X1K5FZLG1N4qnJxAxj1D7VB7gk9
Decimals:  9

Signature: 382YHasjtx9CCpu55hL9x8s3ZeY77Jmw9RKFd3y7SSf9HXqw7kUToMLQLuaUMf6mMZqs65rBPNaYhmQqwejntaKG
```

"Address" here is the token mint account - it contains the token's metadata and
can be used to mint new tokens - copy this. You can change the metadata, such as
the decimal count, later on.

### Mint and transfer tokens

All tokens created by `spl-token` have no supply by default - zero of them
exist. To mint new tokens, you have to first create a token account to hold
them. Create it by running:

```bash
# spl-token create-account <token-mint-address>
spl-token create-account FpFppjxbnSwX7kBX9X1K5FZLG1N4qnJxAxj1D7VB7gk9
```

This creates a new token account for your wallet for the token you just created,
printing out the address:

```bash
Creating account DkoGBArBHqNfbgdzrCAufe51uvn7SWSnSKEpwDYcU7F2

Signature: 4HEqkQY3PM1g9XKbpZ6PfKX3i55nGbg2YnfQcdhyB64JMtXPQSyEFCBSUHpSXF2stsRjkQ1caaYofSrVE73VvdWR
```

Next, mint some tokens for yourself by running:

```bash
# spl-token mint <token-mint-address> <amount>
spl-token mint FpFppjxbnSwX7kBX9X1K5FZLG1N4qnJxAxj1D7VB7gk9 1000
```

This will mint 1000 tokens into the token account you just created for your
wallet.

You can now transfer tokens from your wallet to another wallet by running:

```bash
spl-token transfer --fund-recipient <token-mint-address> <amount> <destination-address>
```

You can check the balance of your wallet for any token using:

```bash
spl-token balance <token-mint-address>
```

To view all the token accounts you own and their balances:

```bash
spl-token accounts
```

## Summary

Here are all the commands you need to run to create a token and mint it:

```bash
spl-token create-token
spl-token create-account <token-mint-address>
spl-token mint <token-mint-address> <amount>
```

And to transfer tokens:

```bash
spl-token transfer --fund-recipient <token-mint-address> <amount> <destination-address>
```

## Next steps

You now know how to use the `spl-token` CLI to mint a token. It's possible to do
this and much more with the `spl-token` Typescript library.

- [Learn more about the SPL token program, the CLI, and the Typescript library](https://spl.solana.com/token)
- [Update the metadata of your token, such as the name and symbol](https://docs.metaplex.com/programs/token-metadata/overview)
- [Create and deploy a Solana Rust program](./rust.md)
