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
- How to interact with your on chain program using JavaScript

## Using Solana Playground

[Solana Playground](https://beta.solpg.io) is browser based application that will let you write, build, and deploy on chain Solana programs. All from your browser. No installation needed.

It is a great developer resources for getting started with Solana development, especially on Windows.

### Import our example project

In a new tab in your browser, open our example "_Hello World_" project on Solana Playground: https://beta.solpg.io/6314a69688a7fca897ad7d1d

Next, import the project into your local workspace by clicking the "**Import**" icon and naming your project `hello_world`.

![Import the get started Solana program on Solana Playground](/img/quickstarts/solana-get-started-import-on-playground.png)

> If you do **not** import the program into **your** Solana Playground, then you will **not** be able to make changes to the code. But you **will** still be able to build and deploy the code to a Solana cluster.

### Create a Playground wallet

Normally with [local development](./local.md), you will need to create a file system wallet for use with the Solana CLI. But with the Solana Playground, you only need to click a few buttons to create a browser based wallet.

> **Note:**
> Your _Playground Wallet_ will be saved in your browser's local storage. Clearing your browser cache will remove your saved wallet. When creating a new wallet, you will have the option to save a local copy of your wallet's keypair file.

Click on the red status indicator button at the bottom left of the screen, (optionally) save your wallet's keypair file to your computer for backup, then click "**Continue**".

After your Playground Wallet is created, you will notice the bottom of the window now states your wallet's address, your SOL balance, and the Solana cluster you are connected to (Devnet is usually the default/recommended, but a "localhost" [test validator](./local.md) is also acceptable).

## Create a Solana program

The code for your Rust based Solana program will live in your `src/lib.rs` file. Inside `src/lib.rs` you will be able to import your Rust crates and define your logic. Open your `src/lib.rs` file within Solana Playground.

### Import the `solana_program` crate

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

### Write your program logic

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

### Build your program

On the left sidebar, select the "**Build & Deploy**" tab. Next, click the "Build" button.

If you look at the Playground's terminal, you should see your Solana program begin to compile. Once complete, you will see a success message.

![Viewing a successful build of your Rust based program](/img/quickstarts/solana-get-started-successful-build.png)

> Note:
> You may receive _warning_ when your program is compiled due to unused variables. Don't worry, these warning will not affect your build. They are due to our very simple program not using all the variables we declared in the `process_instruction` function.

### Deploy your program

You can click the "Deploy" button to deploy your first program to the Solana blockchain. Specifically to your selected cluster (e.g. Devnet, Testnet, etc).

After each deployment, you will see your Playground Wallet balance change. By default, Solana Playground will automatically request SOL airdrops on your behalf to ensure your wallet has enough SOL to cover the cost of deployment.

![Build and deploy your Solana program to the blockchain](/img/quickstarts/solana-get-started-build-and-deploy.png)

### Find your program id

When executing a program using [web3.js](../developing/clients/javascript-reference.md) or from [another Solana program](../developing/programming-model/calling-between-programs.md), you will need to provide the `program id` (aka public address of your program).

Inside Solana Playground's **Build & Deploy** sidebar, you can find your `program id` under the **Program Credentials** dropdown.

#### Congratulations!

You have successfully setup, built, and deployed a Solana program using the Rust language directly in your browser. Next, we will demonstrate how to interact with your on chain program.

## Interact with your on chain program

Once you have successfully deployed a Solana program to the blockchain, you will want to be able to interact with that program.

Like most developers creating dApps and websites, we will interact with our on chain program using JavaScript inside a NodeJS application. Specifically, will use the open source [NPM package](https://www.npmjs.com/package/@solana/web3.js) `@solana/web3.js` to aid in our client application.

> **NOTE:**
> This web3.js package is an abstraction layer on top of the [JSON RPC API](./../developing/clients/jsonrpc-api.md) that reduced the need for rewriting common boilerplate, helping to simplify your client side application code.

### Initialize a new Node project

For a simple client application for our on chain program, create a new folder for the Node project to live:

```bash
mkdir hello_world && cd hello_world
```

Initialize a new Node project on you local computer:

```bash
npm init -y
# or
yarn init -y
```

To easily interact with the Solana blockchain, add the [`@solana/web3.js`](../developing/clients/javascript-api.md) package to your Node project:

```bash
npm i @solana/web3.js
# or
yarn add @solana/web3.js
```

Create a new file named `app.js` and open it in your favorite code editor. This is where we will work for the rest of our `hello world` program.

### Connect to the cluster

Import the `@solana/web3.js` library into your project so we can use all the JSON RPC helper functions that are built into it:

```js
// import the Solana web3.js library
const web3 = require("@solana/web3.js");
```

We will also define the `SOLANA_CLUSTER` your program was deployed as well as the `PROGRAM_ID` of your new program:

```js
// define our program ID and cluster to interact with
const SOLANA_CLUSTER = "devnet";
const PROGRAM_ID = "5AQaXHRKmoZMuBem544ELfEAN2qe2y9PU8nnezvpgeP7";
```

Next we will need to create a `connection` to the specific cluster we deployed our program to:

```js
// create a new connection to the Solana blockchain
const connection = new web3.Connection(web3.clusterApiUrl(SOLANA_CLUSTER));
```

### Create and fund a wallet for testing

In order to submit [transactions](../developing/programming-model/transactions.md) to a Solana cluster, you will need to have a wallet that will pay for the cost of execution. For simplicity, we will generate a dummy "throw away" wallet (and fund it with SOL) for our sample `hello_world` program named `payer`

```js
// create a "throw away" wallet for testing
let payer = web3.Keypair.generate();
console.log("Generated payer address:", payer.publicKey.toBase58());

// fund the "throw away" wallet via an airdrop
console.log("Requesting airdrop...");
let airdropSignature = await connection.requestAirdrop(
  payer.publicKey,
  web3.LAMPORTS_PER_SOL,
);

// wait for the airdrop to be completed
await connection.confirmTransaction(airdropSignature);

// log the signature to the console
console.log(
  "Airdrop submitted:",
  `https://explorer.solana.com/tx/${airdropSignature}?cluster=${SOLANA_CLUSTER}`,
);
```

> You will notice that we are logging a URL of the Solana Explorer to the console. This will help us view our transaction on the blockchain when we run our Node application.

### Create and send a transaction

To execute your on chain program, you must send a [transaction](../developing/programming-model/transactions.md) to it. Each transaction submitted to the Solana blockchain contains a listing of instructions (and the program's that instruction will interact with).

Here we create a new transaction and add a single `instruction` to it:

```js
// create an empty transaction
const transaction = new web3.Transaction();

// add a single instruction to the transaction
transaction.add(
  new web3.TransactionInstruction({
    keys: [
      {
        pubkey: payer.publicKey,
        isSigner: true,
        isWritable: false,
      },
      {
        pubkey: web3.SystemProgram.programId,
        isSigner: false,
        isWritable: false,
      },
    ],
    programId: new web3.PublicKey(PROGRAM_ID),
  }),
);
```

Each `instruction` must include all the keys involved in the operation and the program ID we want to execute. In this example, our `keys` contain the `payer` (that we created earlier to pay for the execution) and the [System Program](../developing/runtime-facilities/programs#system-program) (which is required for all program execution).

With our transaction created, we can submit it to the cluster (via our `connection`):

```js
// submit the transaction to the cluster
console.log("Sending transaction...");
let txid = await web3.sendAndConfirmTransaction(connection, transaction, [
  payer,
]);
console.log(
  "Transaction submitted:",
  `https://explorer.solana.com/tx/${txid}?cluster=${SOLANA_CLUSTER}`,
);
```

### Run our application

With our Node application written, you can run the code via the command line:

```bash
node app.js
```

Once your Node application completes, you will see output similar to this:

```bash
Generated payer address: 8fBnnMz2SLBcmrsrvWBAt91kSPAWRTkjbrVPJqDcxq2s
Requesting airdrop...
Airdrop complete: https://explorer.solana.com/tx/38EXqADsgAj1yNpcTDRwC66HooW1d9mk9PMnPP9V8JbsokZPuWZhP3W9EaixXhPFoBXVQk1NQ2otCs5W1ukt73Hc?cluster=devnet
Sending transaction...
Transaction submitted: https://explorer.solana.com/tx/3gv6uAkyDoLZR94hcMf3fDiwnDNN9rrdXos9rmMmok2xJtaW9NKehYKqSspxVH2HpAK7WHfRYQfsyuzr9gguzf2j?cluster=devnet
```

> View this example transaction on Solana explorer: https://explorer.solana.com/tx/3gv6uAkyDoLZR94hcMf3fDiwnDNN9rrdXos9rmMmok2xJtaW9NKehYKqSspxVH2HpAK7WHfRYQfsyuzr9gguzf2j?cluster=devnet

Using the Solana explorer link for your "submitted transaction", you can view the message our program logged on chain, via `msg!("Hello, world!")`, in the "**Program Instruction Logs**" section of the Solana Explorer.

![View the on chain instruction logs via Solana explorer](/img/quickstarts/solana-get-started-view-logs-on-explorer.png)

#### Congratulations!!!

You have now written a client application for your on chain program. You are now a Solana developer!

PS: Try to update your program's message then re-build, re-deploy, and re-execute your program.

## Next steps

See the links below to learn more about writing Solana programs:

- [Setup your local development environment](./local.md)
- [Overview of writing Solana programs](../developing/on-chain-programs/overview)
- [Learn more about developing Solana programs with Rust](../developing/on-chain-programs/developing-Rust)
- [Debugging on chain programs](../developing/on-chain-programs/debugging)
