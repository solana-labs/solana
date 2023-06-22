---
title: "Create and send a transaction"
description:
  "This quickstart guide will show you how to build a transaction from scratch
  using the Solana SDK in Javascript."
keywords:
  - Solana SDK
  - install solana SDK
  - create transaction solana
  - solana send transaction
  - create sdk transaction
  - tutorial
  - intro to solana development
  - blockchain developer
  - blockchain tutorial
  - web3 developer
---

This quickstart guide will show you how to use the Solana Javascript SDK to
create, sign and send a transaction on the devnet, including environment setup.

## What you will learn

- How to load a local filesystem wallet using the SDK
- How to interact with custom programs on the devnet 
- How to create instructions that include custom data
- How to use the SDK to create, sign and send a transaction

## Prerequisites

This guide assumes you've completed [local development setup](./local.md) and
have a filesystem wallet set up.

Make sure you have [Node.js](https://nodejs.org/en/) installed. You can check
this by running `node -v` in your terminal. You should see a version number
printed out.

## Set up your environment

### Configure the CLI to use devnet

We'll be using the devnet for this guide. Set the Solana CLI to use the devnet
by running:

```bash
solana config set --url devnet
```

Next, run `solana airdrop 2` to get some SOL for the devnet. If this fails,
reduce the amount by using `solana airdrop 1`. You need less than 0.01 SOL to
send a transaction, check your balance by running `solana balance`.

### Setup a local client

Start by creating a node project and installing the Solana SDK. Run this in your
terminal:

```bash
mkdir create-tx
cd create-tx
npm init -y
npm install @solana/web3.js
```

Open the `solana-tx` directory in your favorite code editor.

## Connect to the devnet and load a local wallet

Create a file called `main.js` and add the following code:

```js
const web3 = require("@solana/web3.js");
const os = require("os");
const fs = require("fs");
const path = require("path");

const CLUSTER = web3.clusterApiUrl("devnet");
const connection = new web3.Connection(CLUSTER, "confirmed");
const PROGRAM_ID = new web3.PublicKey(
  "7UCpQWEgiX7nv4sibshx93JfEictrRcgrjhgTqGn77XK",
);

const keypairPath = path.join(os.homedir(), ".config", "solana", "id.json");
let keypair;

try {
  keypair = web3.Keypair.fromSecretKey(
    Buffer.from(JSON.parse(fs.readFileSync(keypairPath, "utf-8"))),
  );
} catch (err) {
  console.error("Error while loading the keypair:", err);
  process.exit(1);
}

console.log("Local keypair address:", keypair.publicKey.toBase58());
```

This code imports the Solana SDK, creates a connection to the Solana Devnet, and
loads the keypair from the filesystem wallet. The `PROGRAM_ID` provided is for a
program deployed on the devnet that echoes back whatever data we send to it.

Running the script as it is with `node main.js` will print out the address of
your local keypair.

## Create a transaction

We're now ready to create a transaction. Add this function below your existing
code:

```js
async function echoProgram(connection, payer, programId) {
  // Format instruction data as a buffer of UTF-8 bytes
  const instructionData = Buffer.from("Hello Solana Devnet");

  const instruction = new web3.TransactionInstruction({
    // An array of addresses that the instruction will read from, or write to
    keys: [
      {
        pubkey: payer.publicKey,
        isSigner: true,
        isWritable: false,
      },
    ],

    // The program we're interacting with
    programId,

    // Instruction data in bytes
    data: instructionData,
  });

  // You can create another instruction here if you want to echo a second time or interact with another program

  // Create a transaction and add the instruction we just defined
  // We can add multiple instructions here that are run sequentially
  const transaction = new web3.Transaction().add(instruction);

  const signature = await web3.sendAndConfirmTransaction(
    connection,
    transaction,
    [payer],
  );

  console.log(
    `You can view your transaction on the Solana Transaction https://explorer.solana.com/tx/${signature}?cluster=devnet`,
  );
}

echoProgram(connection, keypair, PROGRAM_ID).catch(console.error);
```

This function creates, signs and sends a transaction to the devnet. The
transaction contains an instruction to interact with the echo program we defined
earlier.

### Build the instruction

The first thing we're doing is formatting our instruction data into a buffer of
UTF-8 bytes. While UTF-8 itself is not "binary", it is a way of converting
something (like text) into a binary format that can be more efficiently stored
and transmitted by computer systems. This is called "serialization".

This is done for a few reasons:

- Speed: It's more efficient to store and transmit data in bytes
- Interoperability: Programs written in different languages can interact with
  each other because they all use the same format
- Uniformity: It's easier to work with data in a consistent format
- Security: It's more secure to use a consistent format to prevent ambiguity in
  interpretation of the data

The instruction data is then passed to the `TransactionInstruction` constructor
along with the program ID and the keypair of the payer. The payer is the account
that will pay for the transaction fees. In this case, we're using the keypair
from our local wallet.

### Create, sign and send the transaction

Next, we create a transaction and add the instruction we just defined. We can
add multiple instructions here that are run sequentially. Finally, we sign and
send the transaction to the devnet.

Run this script with `node main.js` and you should see a link to the transaction
on the Solana Explorer. Scroll down to the bottom and you'll see your message in
the program instruction logs!

    > Program logged: "Received data: [72, 101, 108, 108, 111, 32, 83, 111, 108, 97, 110, 97, 32, 68, 101, 118, 110, 101, 116]"
    > Program logged: "Echo: Hello Solana Devnet"
    > Program consumed: 6837 of 200000 compute units
    > Program returned success

### Congratulations!

You can now build transactions that take in custom instruction data! The
majority of blockchain development is interacting with existing programs. You
can build hundreds of apps that just interact with all the programs already out
there.

## Next steps

Check out these links to learn more about transactions and programs:

- [Learn more about the Solana Javascript SDK](../developing/clients/javascript-reference.md)
- [Check out the web3-examples repo for common transactions](https://github.com/solana-developers/web3-examples)
- [Learn more about transactions](../developing/programming-model/transactions)
- [Deploy your own Rust program](./rust.md)
