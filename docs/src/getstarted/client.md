---
title: "Create a dApp client"
description:
  "This quickstart guide will show you how to set up a client that lets anyone
  interact with a custom program on the Solana Devnet"
keywords:
  - Solana client
  - Nextjs app Solana
  - Create Solana dApp
  - Build Solana client
  - Deploy the Solana dApp scaffold
  - deploy a Solana dApp
  - Solana dApp tutorial
  - intro to Solana development
---

This quickstart guide will show you how to set up a client that lets anyone
interact with an existing program on the Solana Devnet. We'll use the
[create-solana-dapp](https://github.com/solana-developers/create-solana-dapp)
CLI to set up a Next.js app and configure it to send a message to the Echo
program.

## What you will learn

- How to use `create-solana-dapp` to set up a client
- Configuring the Solana dApp scaffold
- Interacting with programs on the devnet
- Sending instruction data from clients to programs

## Prerequisites

Make sure you have [Node.js](https://nodejs.org/en/) installed. You can check
that it's available by running `node -v` in your terminal. You should see a
version number printed out.

## Set up the client

### The Solana dApp scaffold

The easiest and quickest way to get started building a web app client is using
one of the available dApp scaffolds. These are available for Next.js, Vue and
Svelte. We'll use the Next.js scaffold for this guide.

Start by setting up the Next.js scaffold using `create-solana-dapp` by running
this in your terminal:

```bash
npx create-solana-dapp solana-dapp
```

The first time you run this, it will ask you if you want to install
`create-solana-dapp`. Enter `y` and hit enter. This will install the CLI
globally on your machine and run it. It may take a few minutes as it will also
install all the dependencies for the scaffold.

Open up the `solana-dapp` folder in your code editor and navigate to the `app`
folder inside it. You can ignore the `program` folder for now. Start the
template by running `npm run dev` in your terminal. This will start the Next.js
app on `localhost:3000`.

### The wallet adapter

The [Solana Wallet adapter](https://github.com/solana-labs/wallet-adapter#usage)
is a library that makes it easy to connect to any wallets on Solana that support
the official wallet standard. This means you don't need to implement separate
APIs for different wallets - the wallet adapter will handle that for you.

Open up `src/contexts/ContextProvider.tsx` in your code editor. You'll find the
`ContextProvider` component that wraps the entire app, making the wallet adapter
available to all pages and children components in the app.

The wallet adapter is initialized with a list of wallets that the user can
choose from. By default the scaffold uses the `UnsafeWalletAdapter` which is a
wallet that is only meant for development purposes. Change it to a list of
wallets that you want to support in your app like this:

```tsx
// Line 3
import {
    SolflareWalletAdapter,
} from '@solana/wallet-adapter-wallets';

...

// Line 30
    const wallets = useMemo(
        () => [
            new SolflareWalletAdapter(),
        ],
        [network]
    );
```

This will enable the Solflare wallet in your app as well as any wallets that are
registered as a standard wallet and installed in the user's browser (such as
Phantom and Glow).

## Interacting with programs

The scaffold has several components that you can use to interact with programs
and send transactions. When exploring it, start with just one component and
stick with it so you don't get overwhelmed.

### Set up the `SendEcho` component

To interact with the echo program using the client, we'll need to send a
transaction that is signed and funded by the user. Create a file called
`SendEcho.tsx` in `app/src/components` and add the following code to it:

```tsx
import { useConnection, useWallet } from "@solana/wallet-adapter-react";
import {
  Transaction,
  TransactionSignature,
  TransactionInstruction,
  PublicKey,
} from "@solana/web3.js";
import { FC, useCallback } from "react";
import { notify } from "../utils/notifications";

export const SendEcho: FC = () => {
  const { connection } = useConnection();
  const { publicKey, sendTransaction } = useWallet();
  const programId = new PublicKey(
    "7UCpQWEgiX7nv4sibshx93JfEictrRcgrjhgTqGn77XK",
  );

  const onClick = useCallback(async () => {
    // Transaction code will go here
  }, []);

  return (
    <div className="flex flex-row justify-center">
      <div className="relative group items-center">
        <div
          className="m-1 absolute -inset-0.5 bg-gradient-to-r from-indigo-500 to-fuchsia-500 
                rounded-lg blur opacity-20 group-hover:opacity-100 transition duration-1000 group-hover:duration-200 animate-tilt"
        ></div>
        <button
          className="group w-60 m-2 btn animate-pulse bg-gradient-to-br from-indigo-500 to-fuchsia-500 hover:from-white hover:to-purple-300 text-black"
          onClick={onClick}
          disabled={!publicKey}
        >
          <div className="hidden group-disabled:block ">
            Wallet not connected
          </div>
          <span className="block group-disabled:hidden">Send Echo</span>
        </button>
      </div>
    </div>
  );
};
```

This has the same styling and structure as the other transaction components in
the scaffold. The imports have been updated for this transaction and the
`programId` of the Echo program has been added.

To test it, we'll put it in `app/src/views/basics/index.tsx` alongside the other
basics:

```tsx
import { FC } from "react";
import { SignMessage } from "../../components/SignMessage";
import { SendTransaction } from "../../components/SendTransaction";
import { SendVersionedTransaction } from "../../components/SendVersionedTransaction";
import { SendEcho } from "../../components/SendEcho";

export const BasicsView: FC = ({}) => {
  return (
    <div className="md:hero mx-auto p-4">
      <div className="md:hero-content flex flex-col">
        <h1 className="text-center text-5xl font-bold text-transparent bg-clip-text bg-gradient-to-br from-indigo-500 to-fuchsia-500 mt-10 mb-8">
          Basics
        </h1>
        <div className="text-center">
          <SignMessage />
          <SendTransaction />
          <SendVersionedTransaction />
          {/* Added here */}
          <SendEcho />
        </div>
      </div>
    </div>
  );
};
```

Head over to `localhost:3000/basics` and you'll see the `Send Echo` button. It
does nothing right now as we haven't added the transaction code.

### Create a transaction on the client

The code for this will be largely the same as the transaction quickstart,
however, instead of loading a keypair from a file, we'll use the wallet adapter
to get the user to sign and pay for the transaction.

Add the following code to the `onClick` function on line 18:

```tsx
const onClick = useCallback(async () => {
  if (!publicKey) {
    notify({ type: "error", message: `Wallet not connected!` });
    console.log("error", `Send Transaction: Wallet not connected!`);
    return;
  }

  let signature: TransactionSignature = "";
  const message = prompt("Enter message for the blockchain:");

  if (!message) {
    notify({ type: "error", message: `No message entered!` });
    console.log("error", `No message entered!`);
    return;
  }

  try {
    // Format the message as bytes
    const messageBytes = Buffer.from(message);

    console.log("Message bytes:", messageBytes);

    const instructions = new TransactionInstruction({
      keys: [{ pubkey: publicKey, isSigner: true, isWritable: false }],
      programId,
      data: messageBytes,
    });

    let latestBlockhash = await connection.getLatestBlockhash();

    const transaction = new Transaction().add(instructions);

    signature = await sendTransaction(transaction, connection);

    await connection.confirmTransaction(
      { signature, ...latestBlockhash },
      "confirmed",
    );

    notify({
      type: "success",
      message: "Transaction successful!",
      txid: signature,
    });
  } catch (error: any) {
    notify({
      type: "error",
      message: `Transaction failed!`,
      description: error?.message,
      txid: signature,
    });
    console.log("error", `Transaction failed! ${error?.message}`, signature);
    return;
  }
}, [publicKey, notify, connection, sendTransaction]);
```

This asks the user for a message they want to send to the echo program, formats
it, creates a transaction and prompts the user to approve it via their wallet.

Head over to the basics page and try it out. You should see a success toast
message on the bottom left corner on your screen which contains a link to the
transaction.

You now know how to create a transaction with custom instruction data via a
client!

## Next steps

The scaffold is loaded with useful components and utilities that you will find
useful when building dapps. Check out the `SignMessage.tsx` component to see how
to sign messages with the wallet adapter, or see the links below to learn more
about Solana clients:

- [Read more about the Solana Wallet Adapter in the Scaffold ](https://solana.com/news/solana-scaffold-part-1-wallet-adapter)
- [Learn more about the Solana Javascript SDK](../developing/clients/javascript-reference.md)
- [Check out the web3-examples repo for common transactions](https://github.com/solana-developers/web3-examples)
- [Learn more about transactions](../developing/programming-model/transactions)
