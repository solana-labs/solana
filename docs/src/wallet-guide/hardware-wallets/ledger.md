---
title: Ledger Nano
---

This page describes how to use a Ledger Nano S, Nano S Plus, or Nano X to interact with Solana
using the command line tools.

## Before You Begin

- [Set up a Nano with the Solana App](https://support.ledger.com/hc/en-us/articles/360016265659-Solana-SOL-?docs=true)
- [Install the Solana command-line tools](../../cli/install-solana-cli-tools.md)

## Use Ledger Nano with Solana CLI

1. Ensure the Ledger Live application is closed
2. Plug your Nano into your computer's USB port
3. Enter your pin and start the Solana app on the Nano
4. Ensure the screen reads "Application is ready"

### View your Wallet ID

On your computer, run:

```bash
solana-keygen pubkey usb://ledger
```

This confirms your Ledger device is connected properly and in the correct state
to interact with the Solana CLI. The command returns your Ledger's unique
_wallet ID_. When you have multiple Nano devices connected to the same
computer, you can use your wallet ID to specify which Ledger hardware wallet
you want to use. If you only plan to use a single Nano on your computer
at a time, you don't need to include the wallet ID. For information on
using the wallet ID to use a specific Ledger, see
[Manage Multiple Hardware Wallets](#manage-multiple-hardware-wallets).

### View your Wallet Addresses

Your Nano supports an arbitrary number of valid wallet addresses and signers.
To view any address, use the `solana-keygen pubkey` command, as shown below,
followed by a valid [keypair URL](../hardware-wallets.md#specify-a-keypair-url).

Multiple wallet addresses can be useful if you want to transfer tokens between
your own accounts for different purposes, or use different keypairs on the
device as signing authorities for a stake account, for example.

All of the following commands will display different addresses, associated with
the keypair path given. Try them out!

```bash
solana-keygen pubkey usb://ledger
solana-keygen pubkey usb://ledger?key=0
solana-keygen pubkey usb://ledger?key=1
solana-keygen pubkey usb://ledger?key=2
```

- NOTE: keypair url parameters are ignored in **zsh**
  &nbsp;[see troubleshooting for more info](#troubleshooting)

You can use other values for the number after `key=` as well.
Any of the addresses displayed by these commands are valid Solana wallet
addresses. The private portion associated with each address is stored securely
on the Nano, and is used to sign transactions from this address.
Just make a note of which keypair URL you used to derive any address you will be
using to receive tokens.

If you are only planning to use a single address/keypair on your device, a good
easy-to-remember path might be to use the address at `key=0`. View this address
with:

```bash
solana-keygen pubkey usb://ledger?key=0
```

Now you have a wallet address (or multiple addresses), you can share any of
these addresses publicly to act as a receiving address, and you can use the
associated keypair URL as the signer for transactions from that address.

### View your Balance

To view the balance of any account, regardless of which wallet it uses, use the
`solana balance` command:

```bash
solana balance SOME_WALLET_ADDRESS
```

For example, if your address is `7cvkjYAkUYs4W8XcXsca7cBrEGFeSUjeZmKoNBvEwyri`,
then enter the following command to view the balance:

```bash
solana balance 7cvkjYAkUYs4W8XcXsca7cBrEGFeSUjeZmKoNBvEwyri
```

You can also view the balance of any account address on the Accounts tab in the
[Explorer](https://explorer.solana.com/accounts)
and paste the address in the box to view the balance in you web browser.

Note: Any address with a balance of 0 SOL, such as a newly created one on your
Ledger, will show as "Not Found" in the explorer. Empty accounts and non-existent
accounts are treated the same in Solana. This will change when your account
address has some SOL in it.

### Send SOL from a Nano

To send some tokens from an address controlled by your Nano, you will
need to use the device to sign a transaction, using the same keypair URL you
used to derive the address. To do this, make sure your Nano is plugged in,
unlocked with the PIN, Ledger Live is not running, and the Solana App is open
on the device, showing "Application is Ready".

The `solana transfer` command is used to specify to which address to send tokens,
how many tokens to send, and uses the `--keypair` argument to specify which
keypair is sending the tokens, which will sign the transaction, and the balance
from the associated address will decrease.

```bash
solana transfer RECIPIENT_ADDRESS AMOUNT --keypair KEYPAIR_URL_OF_SENDER
```

Below is a full example. First, an address is viewed at a certain keypair URL.
Second, the balance of that address is checked. Lastly, a transfer transaction
is entered to send `1` SOL to the recipient address `7cvkjYAkUYs4W8XcXsca7cBrEGFeSUjeZmKoNBvEwyri`.
When you hit Enter for a transfer command, you will be prompted to approve the
transaction details on your Ledger device. On the device, use the right and
left buttons to review the transaction details. If they look correct, click
both buttons on the "Approve" screen, otherwise push both buttons on the "Reject"
screen.

```bash
~$ solana-keygen pubkey usb://ledger?key=42
CjeqzArkZt6xwdnZ9NZSf8D1CNJN1rjeFiyd8q7iLWAV

~$ solana balance CjeqzArkZt6xwdnZ9NZSf8D1CNJN1rjeFiyd8q7iLWAV
1.000005 SOL

~$ solana transfer 7cvkjYAkUYs4W8XcXsca7cBrEGFeSUjeZmKoNBvEwyri 1 --keypair usb://ledger?key=42
Waiting for your approval on Ledger hardware wallet usb://ledger/2JT2Xvy6T8hSmT8g6WdeDbHUgoeGdj6bE2VueCZUJmyN
✅ Approved

Signature: kemu9jDEuPirKNRKiHan7ycybYsZp7pFefAdvWZRq5VRHCLgXTXaFVw3pfh87MQcWX4kQY4TjSBmESrwMApom1V
```

After approving the transaction on your device, the program will display the
transaction signature, and wait for the maximum number of confirmations (32)
before returning. This only takes a few seconds, and then the transaction is
finalized on the Solana network. You can view details of this or any other
transaction by going to the Transaction tab in the
[Explorer](https://explorer.solana.com/transactions)
and paste in the transaction signature.

## Advanced Operations

### Manage Multiple Hardware Wallets

It is sometimes useful to sign a transaction with keys from multiple hardware
wallets. Signing with multiple wallets requires _fully qualified keypair URLs_.
When the URL is not fully qualified, the Solana CLI will prompt you with
the fully qualified URLs of all connected hardware wallets, and ask you to
choose which wallet to use for each signature.

Instead of using the interactive prompts, you can generate fully qualified
URLs using the Solana CLI `resolve-signer` command. For example, try
connecting a Nano to USB, unlock it with your pin, and running the
following command:

```text
solana resolve-signer usb://ledger?key=0/0
```

You will see output similar to:

```text
usb://ledger/BsNsvfXqQTtJnagwFWdBS7FBXgnsK8VZ5CmuznN85swK?key=0/0
```

but where `BsNsvfXqQTtJnagwFWdBS7FBXgnsK8VZ5CmuznN85swK` is your `WALLET_ID`.

With your fully qualified URL, you can connect multiple hardware wallets to
the same computer and uniquely identify a keypair from any of them.
Use the output from the `resolve-signer` command anywhere a `solana` command
expects a `<KEYPAIR>` entry to use that resolved path as the signer for that
part of the given transaction.

## Troubleshooting

### Keypair URL parameters are ignored in zsh

The question mark character is a special character in zsh. If that's not a
feature you use, add the following line to your `~/.zshrc` to treat it as a
normal character:

```bash
unsetopt nomatch
```

Then either restart your shell window or run `~/.zshrc`:

```bash
source ~/.zshrc
```

If you would prefer not to disable zsh's special handling of the question mark
character, you can disable it explicitly with a backslash in your keypair URLs.
For example:

```bash
solana-keygen pubkey usb://ledger\?key=0
```

## Support

Check out our [Wallet Support Page](../support.md)
for ways to get help.

Read more about [sending and receiving tokens](../../cli/transfer-tokens.md) and
[delegating stake](../../cli/delegate-stake.md). You can use your Ledger keypair URL
anywhere you see an option or argument that accepts a `<KEYPAIR>`.
