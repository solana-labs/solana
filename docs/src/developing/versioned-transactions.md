---
title: Versioned Transactions
description: ""
# keywords:
#   -
---

[Versioned Transactions](./versioned-transactions.md) are the new transaction format that allow for additional functionality in the Solana runtime, including [Address Lookup Tables](./lookup-tables.md).

While changes to [on chain](./on-chain-programs/overview.md) programs are **NOT** required to support the new functionality of versioned transactions (or for backwards compatibility), developers **WILL** need update their client side code to prevent [errors due to different transaction versions](#max-supported-transaction-version).

## Current Transaction Versions

The Solana runtime supports two transaction versions:

- `legacy` - older transaction format with no additional benefit
- `0` - added support for [Address Lookup Tables](./lookup-tables.md)

## Max supported transaction version

All RPC requests that return a transaction **_should_** specify the highest version of transactions they will support in their application using the `maxSupportedTransactionVersion` option, including [`getBlock`](/api#getblock) and [`getTransaction`](/api#gettransaction).

An RPC request will fail if a [Versioned Transaction](./versioned-transactions.md) is returned that is higher than the set `maxSupportedTransactionVersion`. (i.e. if a version `0` transaction is returned when `legacy` is selected)

> WARNING:
> If no `maxSupportedTransactionVersion` value is set, then only `legacy` transactions will be allowed in the RPC response. Therefore, your RPC requests **WILL** fail if any version `0` transactions are returned.

## How to set max supported version

You can set the `maxSupportedTransactionVersion` using both the [`@solana/web3.js`](https://solana-labs.github.io/solana-web3.js/) library and JSON formatted requests directly to an RPC endpoint.

### Using web3.js

Using the [`@solana/web3.js`](https://solana-labs.github.io/solana-web3.js/) library, you can retrieve the most recent block or get a specific transaction:

```js
// connect to the `devnet` cluster and get the current `slot`
const connection = new web3.Connection(web3.clusterApiUrl("devnet"));
const slot = await connection.getSlot();

// get the latest block (allowing for v0 transactions)
const block = await connection.getBlock(slot, {
  maxSupportedTransactionVersion: 0,
});

// get a specific transaction (allowing for v0 transactions)
const getTx = await connection.getTransaction(
  "3jpoANiFeVGisWRY5UP648xRXs3iQasCHABPWRWnoEjeA93nc79WrnGgpgazjq4K9m8g2NJoyKoWBV1Kx5VmtwHQ",
  {
    maxSupportedTransactionVersion: 0,
  },
);
```

### JSON requests to the RPC

Using a standard JSON formatted POST request, you can set the `maxSupportedTransactionVersion` when retrieving a specific block:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d \
'{"jsonrpc": "2.0", "id":1, "method": "getBlock", "params": [430, {
  "encoding":"json",
  "maxSupportedTransactionVersion":0,
  "transactionDetails":"full",
  "rewards":false
}]}'
```

## How create a Versioned Transaction

Versioned transactions can be created similar to the older method of creating transactions. There are differences in using certain libraries that should be noted.

Below is an example of how to create a Versioned Transaction, using the `@solana/web3.js` library, to send perform a SOL transfer between two accounts.

#### Notes:

- `payer` is a valid `Keypair` wallet, funded with SOL
- `toAccount` a valid `Keypair`

Firstly, import the web3.js library and create a `connection` to your desired cluster.

We then define the recent `blockhash` and `minRent` we will need for our transaction and the account:

```js
const web3 = require("@solana/web3.js");

// connect to the cluster and get the minimum rent for rent exempt status
const connection = new web3.Connection(web3.clusterApiUrl("devnet"));
let minRent = await connection.getMinimumBalanceForRentExemption(0);
let blockhash = await connection
  .getLatestBlockhash()
  .then((res) => res.blockhash);
```

Create an `array` of all the `instructions` you desire to send in your transaction. In this example below, we are creating a simple SOL transfer instruction:

```js
// create an array with your desires `instructions`
const instructions = [
  web3.SystemProgram.transfer({
    fromPubkey: payer.publicKey,
    toPubkey: toAccount.publicKey,
    lamports: minRent,
  }),
];
```

Next, construct a `MessageV0` formatted transaction message with your desired `instructions`:

```js
// create v0 compatible message
const messageV0 = new web3.TransactionMessage({
  payerKey: payer.publicKey,
  recentBlockhash: blockhash,
  instructions,
}).compileToV0Message();
```

Then, create a new `VersionedTransaction`, passing in our v0 compatible message:

```js
const transaction = new web3.VersionedTransaction(messageV0);

// sign your transaction with the required `Signers`
transaction.sign([payer]);
```

You can sign the transaction by either:

- passing an array of `signatures` into the `VersionedTransaction` method, or
- call the `transaction.sign()` method, passing an array of the required `Signers`

> NOTE:
> After calling the `transaction.sign()` method, all the previous transaction `signatures` will be fully replaced by new signatures created from the provided in `Signers`.

After your `VersionedTransaction` has been signed by all required accounts, you can send it to the cluster and `await` the response:

```js
// send our v0 transaction to the cluster
const txid = await connection.sendTransaction(transaction);
console.log(`https://explorer.solana.com/tx/${txid}?cluster=devnet`);
```

> NOTE:
> Unlike `legacy` transactions, sending a `VersionedTransaction` via `sendTransaction` does **NOT** support transaction signing via passing in an array of `Signers` as the second parameter. You will need to sign the transaction before calling `connection.sendTransaction()`.

## More Resources

- using [Versioned Transactions for Address Lookup Tables](./lookup-tables.md#how-to-create-an-address-lookup-table)
- view an [example of a v0 transaction](https://explorer.solana.com/tx/3jpoANiFeVGisWRY5UP648xRXs3iQasCHABPWRWnoEjeA93nc79WrnGgpgazjq4K9m8g2NJoyKoWBV1Kx5VmtwHQ/?cluster=devnet) on Solana Explorer
- read the [accepted proposal](./../proposals/versioned-transactions.md) for Versioned Transaction and Address Lookup Tables
