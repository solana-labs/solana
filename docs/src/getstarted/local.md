---
title: "Local Development Quickstart"
description: "This quickstart guide will demonstrate how to quickly install and setup your local Solana development environment."
keywords:
  - rust
  - cargo
  - toml
  - program
  - tutorial
  - intro to solana development
  - blockchain developer
  - blockchain tutorial
  - web3 developer
---

This quickstart guide will demonstrate how to quickly install and setup your local development environment, getting you ready to start developing and deploying Solana programs to the blockchain.

## What you will learn

- How to install the Solana CLI locally
- How to setup a localhost Solana cluster/validator
- How to create a Solana wallet for developing
- How to airdrop SOL tokens for your wallet

## Install the Solana CLI

To interact with the Solana clusters from your terminal, install the [Solana CLI tool suite](./../cli/install-solana-cli-tools) on your local system:

```bash
sh -c "$(curl -sSfL https://release.solana.com/stable/install)"
```

## Setup a localhost blockchain cluster

The Solana CLI comes with the [test validator](./../developing/test-validator.md) built in. This command line tool will allow you to run a full blockchain cluster on your machine.

```bash
solana-test-validator
```

> **PRO TIP:**
> Run the Solana test validator in a new/separate terminal window that will remain open. The command line program must remain running for your localhost cluster to remain online and ready for action.

Configure your Solana CLI to use your localhost validator for all your future terminal commands:

```bash
solana config set --url localhost
```

At any time, you can view your current Solana CLI configuration settings:

```bash
solana config get
```

## Create a file system wallet

To deploy a program with Solana CLI, you will need a Solana wallet with SOL tokens to pay for the cost of transactions.

Let's create a simple file system wallet for testing:

```bash
solana-keygen new
```

By default, the `solana-keygen` command will create a new file system wallet located at `~/.config/solana/id.json`. You can manually specify the output file location using the `--outfile /path` option.

> **NOTE:**
> If you already have a file system wallet saved at the default location, this command will **NOT** override it (unless you explicitly force override using the `--force` flag).

#### Set your new wallet as default

With your new file system wallet created, you must tell the Solana CLI to use this wallet to deploy and take ownership of your on chain program:

```bash
solana config set -k ~/.config/solana/id.json
```

## Airdrop SOL tokens to your wallet

Once your new wallet is set as the default, you can request a free airdrop of SOL tokens to it:

```bash
solana airdrop 2
```

> **NOTE:**
> The `solana airdrop` command has a limit of how many SOL tokens can be requested _per airdrop_ for each cluster (localhost, testnet, or devent). If your airdrop transaction fails, lower your airdrop request quantity and try again.

You can check your current wallet's SOL balance any time:

```bash
solana balance
```

## Next steps

See the links below to learn more about writing Rust based Solana programs:

- [Create and deploy a Solana Rust program](./rust.md)
- [Overview of writing Solana programs](../developing/on-chain-programs/overview)
