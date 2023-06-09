---  
title: "CLI & SDK quickstart"
description: 'This quickstart guide will show you how to use the CLI and the SDK to send SOL.'
keywords:
  - solana cli send transaction
  - send SOL using SDK
  - transfer SOL using CLI
  - set up Solana Javascript SDK
  - set up Solana web3 API 
  - tutorial
  - intro to solana development
  - blockchain developer
  - blockchain tutorial
  - web3 developer
---

This quickstart guide will show you how to use the Solana CLI and the Javascript SDK to send SOL, including environment setup.

## What you will learn

- How to use the Solana CLI to transfer SOL
- How to set up the Javascript SDK
- How to load a local filesystem wallet using the SDK
- How to use the SDK to transfer SOL

## Prerequisites
This guide assumes you've completed [local development setup](./local.md) and have a filesystem wallet with SOL in it. Running `solana config get` should output the following:

```bash
$ solana config get
Config File: /Users/NAME/.config/solana/cli/config.yml
RPC URL: http://localhost:8899 
WebSocket URL: ws://localhost:8900/ (computed)
Keypair Path: /Users/NAME/.config/solana/id.json 
Commitment: confirmed 
```
Make sure you have `solana-test-validator` running in one terminal window. Running `solana balance` in a second window should print out:

    2 SOL

## Use the CLI to transfer SOL
We'll start by transferring SOL from one account to another using the CLI. 

### Create a new wallet keypair
You need a recipient address to send SOL to. Create a new keypair and record its public key:

```bash
solana-keygen new --no-passphrase --no-outfile
```

The output will contain the address after the text pubkey:.

```bash
pubkey: GKvqsuNcnwWqPzzuhLmGi4rzzh55FhJtGizkhHaEJqiV
```

### Send SOL to the new address
We'll use the `transfer` command to send 0.05 SOL to the new address. 

```bash
# solana transfer <RECIPIENT_ADDRESS> <AMOUNT>
solana transfer GKvqsuNcnwWqPzzuhLmGi4rzzh55FhJtGizkhHaEJqiV 0.05
```

This will give you an error:
```bash
Error: The recipient address (GKvqsuNcnwWqPzzuhLmGi4rzzh55FhJtGizkhHaEJqiV) is not funded. Add `--allow-unfunded-recipient` to complete the transfer
```

The recipient address has not been "initialized" yet: it has never held any SOL and does not have enough for rent, so the Solana network does not know about it yet. 

Add the `--allow-unfunded-recipient` flag to the command to send SOL to it: 

```bash
solana transfer GKvqsuNcnwWqPzzuhLmGi4rzzh55FhJtGizkhHaEJqiV 0.05 --allow-unfunded-recipient
```

This will output a transaction signature. To view transaction details, you can run `solana confirm -v <TRANSACTION_SIGNATURE>`.

Finally, check the balance of the new address with:
```bash
solana balance GKvqsuNcnwWqPzzuhLmGi4rzzh55FhJtGizkhHaEJqiV
```

You now know how to transfer SOL from one account to another using the CLI!

For more information on the `transfer` command, you can run `solana help transfer` in your terminal.

## Using the SDK to transfer SOL
Transferring SOL using the CLI is impractical for most applications. The Solana SDK is the primary tool used for such tasks. We'll use it to build a script that transfers SOL from one account to another.

### Prerequisites
Make sure you have [NodeJs](https://nodejs.org/en/) installed. You can check this by running `node -v` in your terminal. You should see a version number printed out.

### Setup a local client
Start by creating a node project and installing the Solana SDK. Run this in your terminal:

```bash
mkdir solana-tx
cd solana-tx
npm init -y
npm install @solana/web3.js
```

### Connect to file system wallet
Create a file called `main.js` and add the following code:

```js
const web3 = require('@solana/web3.js');
const os = require('os');
const fs = require('fs');
const path = require('path');

const CLUSTER = 'http://127.0.0.1:8899'; // Can be replaced with 'devnet', 'testnet', or 'mainnet-beta'
const RECIPIENT_ADDRESS = '9B5XszUGdMaxCZ7uSQhPzdks5ZQSmWxrmzCSvtJ6Ns6g';

const connection = new web3.Connection(CLUSTER, 'confirmed');
const recipient = new web3.PublicKey(RECIPIENT_ADDRESS);
```

This code imports the Solana SDK and creates a connection to the local Solana cluster. It also defines the recipient address we'll be sending SOL to, which is the address of the faucet on the devnet. You can swap this out for any other address you want to send SOL to.

### Load the filesystem wallet
Next, we need to load the keypair from the file system wallet. This is the keypair you created when you ran `solana-keygen new` in the local development quickstart. Add this code below the previous block:
```js
// Import the existing keypair from the file system wallet
const keypairPath = path.join(os.homedir(), '.config', 'solana', 'id.json');
let secretKey, keypair;

try {
	secretKey = JSON.parse(fs.readFileSync(keypairPath, 'utf-8'));
	keypair = web3.Keypair.fromSecretKey(Buffer.from(secretKey));
} catch (err) {
	console.error('Error while loading the keypair:', err);
	process.exit(1);
}

console.log('Local keypair address:', keypair.publicKey.toBase58());
```

### Create the transaction and send it
Finally, we'll use the built in transfer function to send 0.05 SOL to the recipient address. 
```js
async function transferSol() {
	console.log('Transferring 0.05 SOL...');

	// Create a transaction and add a system instruction to transfer SOL
	const transaction = new web3.Transaction().add(
		
		// This system instruction takes the sender's address, the recipient's address, and the amount to transfer
		web3.SystemProgram.transfer({
			fromPubkey: keypair.publicKey,
			toPubkey: recipient,
			// The amount of SOL must be in Lamports, the smallest unit of SOL
			lamports: web3.LAMPORTS_PER_SOL * 0.05, 
		})
	);

	try {
		// Sign and send the transaction to the local cluster using the keypair
		const signature = await web3.sendAndConfirmTransaction(
			connection,
			transaction,
			[keypair]
		);

		const newBalance = await connection.getBalance(keypair.publicKey);
		console.log('New balance is', newBalance / web3.LAMPORTS_PER_SOL);
	} catch (err) {
		console.error('Error while transferring:', err);
	}
}

transferSol().catch(console.error);
```

This code creates a transaction with one instruction that transfers 0.05 SOL from the keypair's address to the recipient address. It then signs and sends the transaction to the local cluster. Finally, it prints out the new balance of the keypair's address.

You can run this script with `node main.js`. You should see the following output:
```bash
Local keypair address: 8r97HfVyCprL98Cc1a4QgwAJ7eijqzQwrgDazxdSqLf8
Transferring 0.05 SOL...
New balance is 0.90
```

Check out the [web3-examples](https://github.com/solana-developers/web3-examples) repo for more examples of interacting with Solana programs using the SDK. 

## Going beyond localhost
Everything you've done so far has been on your local machine. The wallet balances will disappear when you exit the terminal running the `solana-test-validator` process. You can repeat these steps on the devnet or the mainnet by changing your RPC URL and starting over:
```bash
# This configures your CLI to use the devnet cluster
solana config set --url devnet
solana airdrop 2
# Repeat steps from the top
```

You will also need to update the `CLUSTER` variable in the SDK example to match the cluster you're using (devnet, testnet, or mainnet-beta).

## Next steps

See the links below to learn more about the CLI and the SDK:

- [Learn more about the CLI](../cli)
- [Learn more about the SDK](../developing/clients/javascript-reference.md)
- [Check out the web3-examples repo](https://github.com/solana-developers/web3-examples)