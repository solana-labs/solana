---
title: "C/C++ Quickstart Guide"
description: "This quickstart guide will demonstrate how to quickly setup, build, and deploy your first C/C++ based Solana program to the blockchain."
keywords: "c lang, cpp, c plus plus, program, tutorial, intro to solana development, blockchain developer, blockchain tutorial, web3 developer"
---

This quickstart guide will demonstrate how to quickly setup, build, and deploy your first C based Solana program to the blockchain.

## What you will learn

- How to install the Solana CLI
- How to setup a localhost Solana cluster/validator
- How to create a Solana wallet for developing
- How to program a basic Solana program in C
- How to build and deploy a Solana Rust program

## Install the Solana CLI

To interact with the Solana clusters from your terminal, install the [Solana CLI tool suite](../../cli/install-solana-cli-tools) on your local system:

```bash
sh -c "$(curl -sSfL https://release.solana.com/stable/install)"
```

## Setup a localhost blockchain cluster

The Solana CLI comes with the [test validator](#) built in. This command line tool will allow you to run a full blockchain cluster on your machine.

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

#### Airdrop SOL tokens to your wallet

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

## Create a new C project

## Create your first Solana program

```c

```

This program will simply [log a message](../on-chain-programs/debugging#logging) of "_Hello, world!_" to the blockchain cluster.

## Build your C program

```bash

```

> **NOTE:**
> After each time you build your Solana program, the above command will output the build path of your compiled program's `.so` file and the default keyfile that will be used for the program's address.

## Deploy your Solana program

Using the Solana CLI, you can deploy your program to your currently selected cluster:

```bash
solana program deploy ./target/deploy/hello_world.so
```

Once your Solana program has been deployed (and the transaction [finalized](../../cluster/commitments.md)), the above command will output your program's public address (aka its "program id").

```bash
# example output
Program Id: EFH95fWg49vkFNbAdw9vy75tM7sWZ2hQbTTUmuACGip3
```

#### Congratulations!

You have successfully setup, built, and deployed a Solana program using the Rust language.

PS: Check your Solana wallet's balance again after you deployed. See how much SOL it cost to deploy your simple program?

## Next steps

See the links below to learn more about writing Rust based Solana programs:

See the links below to learn more about writing Rust based Solana programs:

- [Overview of writing Solana programs](../on-chain-programs/overview)
- [Learn more about developing Solana programs with C/C++](../on-chain-programs/developing-c)
- [Debugging on chain programs](../on-chain-programs/debugging)
