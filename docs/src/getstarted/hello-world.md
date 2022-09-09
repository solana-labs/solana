---
title: "Hello World Quickstart Guide"
description: 'This "hello world" quickstart guide will demonstrate how to setup, build, and deploy your first Solana program in your browser with Solana Playground.'
keywords: "playground, solana pg, on chain, rust, native program, tutorial, intro to solana development, blockchain developer, blockchain tutorial, web3 developer"
---

For this "hello world" quickstart guide, we will use [Solana Playground](https://beta.solpg.io), a browser the based IDE, to develop and deploy our Solana program. To use it, you do **NOT** have to install any software on your computer. Simply open Solana Playground in your browser of choice, and you are ready to write and deploy Solana programs.

## What you will learn

- How to get started with Solana Playground
- How to create a Solana wallet on Playground
- How to program a basic Solana program in Rust
- How to build and deploy a Solana Rust program

## Import our example project

In a new tab in your browser, open our example "_Hello World_" project on Solana Playground: https://beta.solpg.io/6314a69688a7fca897ad7d1d

Next, import the project into your local workspace by clicking the "**Import**" icon and naming your project `hello_world`.

![](/img/quickstarts/solana-get-started-import-on-playground.png)

## Create a Playground wallet

Normally with [local development](./local.md), you will need to create a file system wallet for use with the Solana CLI. But with the Solana Playground, you only need to click a few buttons to create a browser based wallet.

> **Note:**
> Your _Playground Wallet_ will be saved in your browser's local storage. Clearing your browser cache will remove your saved wallet. When creating a new wallet, you will have the option to save a local copy of your wallet's keypair file.

Click on the red status indicator button at the bottom left of the screen, (optionally) save your wallet's keypair file to your computer for backup, then click "**Continue**".

After your Playground Wallet is created, you will notice the bottom of the window now states your wallet's address, your SOL balance, and the Solana cluster you are connected to (Devnet is usually the default/recommended, but a "localhost" [test validator](./local.md) is also acceptable).

## Walkthrough of our Solana program

The code for your Rust based Solana program will live in your `src/lib.rs` file. Inside `src/lib.rs` you will be able to import your Rust crates and define your logic. Open your `src/lib.rs` file within Solana Playground.

At the top of `lib.rs`, we import the `solana-program` crate and bring our needed items into the local namespace:

```rust
use solana_program::{
    account_info::AccountInfo,
    entrypoint,
    entrypoint::ProgramResult,
    pubkey::Pubkey,
    msg,
};
```

Every Solana program must define an `entrypoint` that tells the Solana runtime where to start executing your on chain code. Your program's [entrypoint](../developing/on-chain-programs/developing-rust#program-entrypoint) should provide a public function named `process_instruction`:

```rust
// declare and export the program's entrypoint
entrypoint!(process_instruction);

// program entrypoint's implementation
pub fn process_instruction(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    instruction_data: &[u8]
) -> ProgramResult {
    // log a message to the blockchain
    msg!("Hello, world!");

    // gracefully exit the program
    Ok(())
}
```

Every on chain program should return the `Ok` [result enum](https://doc.rust-lang.org/std/result/) with a value of `()`. This tells the Solana runtime that your program executed successfully without errors.

Our program above will simply [log a message](../developing/on-chain-programs/debugging#logging) of "_Hello, world!_" to the blockchain cluster, then gracefully exit with `Ok(())`.

## Build your program

On the left sidebar, select the "**Build & Deploy**" tab. Next, click the "Build" button.

If you look at the Playground's terminal, you should see your Solana program begin to compile. Once complete, you will see a success message.

![](/img/quickstarts/solana-get-started-successful-build.png)

> Note:
> You may receive _warning_ when your program is compiled due to unused variables. Don't worry, these warning will not affect your build. They are due to our very simple program not using all the variables we declared in the `process_instruction` function.

## Deploy your program

You can click the "Deploy" button to deploy your first program to the Solana blockchain. Specifically to your selected cluster (e.g. Devnet, Testnet, etc).

After each deployment, you will see your Playground Wallet balance change. By default, Solana Playground will automatically request SOL airdrops on your behalf to ensure your wallet has enough SOL to cover the cost of deployment.

![](/img/quickstarts/solana-get-started-build-and-deploy.png)

#### Get your program's public address (aka program id)

When executing a program using [web3.js](../developing/clients/javascript-reference.md) or from [another Solana program](../developing/programming-model/calling-between-programs.md), you will need to provide the program id (aka public address of your program).

Inside Solana Playground's **Build & Deploy** sidebar, you can find your program id under the **Program Credentials** dropdown.

#### Congratulations!

You have successfully setup, built, and deployed a Solana program using the Rust language directly in your browser.

## Next steps

See the links below to learn more about writing Solana programs:

- [Setup your local development environment](./local.md)
- [Overview of writing Solana programs](../developing/on-chain-programs/overview)
- [Learn more about developing Solana programs with Rust](../developing/on-chain-programs/developing-Rust)
- [Debugging on chain programs](../developing/on-chain-programs/debugging)
