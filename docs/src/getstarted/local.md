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

To interact with the Solana network from your terminal, you will need to install the [Solana CLI tool suite](./../cli/install-solana-cli-tools) on your local system.

<details>
<summary>macOS / Linux / Windows Subsystem for Linux (WSL)</summary>
Open your favourite terminal application and install the CLI by running:

```bash
sh -c "$(curl -sSfL https://release.solana.com/stable/install)"
```

Depending on your system, the end of the installer messaging may prompt you to

```bash
Please update your PATH environment variable to include the solana programs:
```

If you get the above message, copy and paste the recommended command below it to update `PATH`

Confirm you have the desired version of `solana` installed by running:

```bash
solana --version
```

After a successful install, `solana-install update` may be used to easily update the Solana software to a newer version at any time.

</details>

<details>
<summary>Windows</summary>

:::caution
[WSL](https://learn.microsoft.com/en-us/windows/wsl/install) is the recommended environment for Windows users.
:::

- Open a Command Prompt (`cmd.exe`) as an Administrator

  - Search for Command Prompt in the Windows search bar. When the Command
    Prompt app appears, right-click and select “Open as Administrator”.
    If you are prompted by a pop-up window asking “Do you want to allow this app to
    make changes to your device?”, click Yes.

- Copy and paste the following command, then press Enter to download the Solana
  installer into a temporary directory:

```bash
cmd /c "curl https://release.solana.com/stable/solana-install-init-x86_64-pc-windows-msvc.exe --output C:\solana-install-tmp\solana-install-init.exe --create-dirs"
```

- Copy and paste the following command, then press Enter to install the latest
  version of Solana. If you see a security pop-up by your system, please select
  to allow the program to run.

```bash
C:\solana-install-tmp\solana-install-init.exe stable
```

- When the installer is finished, press Enter.

- Close the command prompt window and re-open a new command prompt window as a
  normal user
- Confirm you have the desired version of `solana` installed by entering:

```bash
solana --version
```

After a successful install, `solana-install update` may be used to easily update the Solana software to a newer version at any time.
</details>


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

### Set your new wallet as default

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
