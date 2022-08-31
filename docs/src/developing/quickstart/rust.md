---
title: "Solana Quickstart: Rust"
description: ""
keywords: ""
---

intro paragraph. "this is why rust is awesome"

## What you will learn

- How to install Solana and rust locally
- How to setup a localhost Solana cluster/validator
- How to create a Solana wallet for developing
- How to program a basic Solana program in rust
- How to build and deploy a Solana rust program

## Install rust and cargo

To be able to compile rust based Solana programs, install the rust language and cargo (the rust package manager) using [Rustup](https://rustup.rs/):

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

## Install the Solana CLI

To interact with the Solana clusters, install the [Solana CLI tool suite](../../cli/install-solana-cli-tools) on your local system:

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

At any time, you can read your current Solana CLI configuration settings:

```bash
solana config get
```

## Create a file system wallet

You

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

Once your new wallet is set as the default on the Solana CLI, you can request a free airdrop of SOL tokens to your wallet:

```bash
solana airdrop 2
```

> **NOTE:**
> The `solana airdrop` command has a limit of how many SOL tokens can be requested _per airdrop_ for each cluster (localhost, testnet, or devent). If your airdrop transaction fails, lower your airdrop request quantity and try again.

You can check your current wallet's SOL balance any time:

```bash
solana balance
```

## Create your first rust program

```rust

```

## Build your rust program

```bash
cargo build-bpf
```

## Deploy your Solana program

```bash

```

## Next steps

See the links below to learn more about writing Rust based Solana programs:

- [current overview page that should be renamed](../on-chain-programs/overview)
- [Developing Solana programs with rust](../on-chain-programs/developing-rust)
- [Debugging on chain programs](../on-chain-programs/debugging)
