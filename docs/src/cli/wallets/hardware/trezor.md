---
title: Using Trezor Hardware Wallets in the Solana CLI
pagination_label: "Hardware Wallets in the Solana CLI: Trezor"
sidebar_label: Trezor
---

This page describes how to use a Trezor Model T or Trezor Safe 3 device to
interact with Solana using the command line tools.

## Before You Begin

- [Install the Solana command-line tools](../../install.md)
- [Review Trezor and BIP-32](https://trezor.io/learn/a/what-is-bip32)
- [Review Trezor and BIP-44](https://trezor.io/learn/a/what-is-bip44)

## Use Trezor Model T or Trezor Safe 3 with Solana CLI

1. Plug your Trezor Model T or Trezor Safe 3 device into your computer's USB port

### View your Wallet Addresses

On your computer, run:

```bash
solana-keygen pubkey usb://trezor?key=0/0
```

This confirms your Trezor device is connected properly and in the correct state
to interact with the Solana CLI. The command returns your Trezor device's first Solana account's 
external (receiving) wallet address using the [BIP-32](https://trezor.io/learn/a/what-is-bip32) derivation path `m/44'/501'/0'/0'`.

Your Trezor device supports an arbitrary number of valid wallet addresses and signers. To
view any address, use the `solana-keygen pubkey` command, as shown below,
followed by a valid [keypair URL](./index.md#specify-a-keypair-url).

Multiple wallet addresses can be useful if you want to transfer tokens between
your own accounts for different purposes, or use different keypairs on the
device as signing authorities for a stake account, for example.

All of the following commands will display different addresses, associated with
the keypair path given. Try them out!

```bash
solana-keygen pubkey usb://trezor?key=0/0
solana-keygen pubkey usb://trezor?key=0/1
solana-keygen pubkey usb://trezor?key=1/0
solana-keygen pubkey usb://trezor?key=1/1
```

- NOTE: keypair url parameters are ignored in **zsh**
  &nbsp;[see troubleshooting for more info](#troubleshooting)

You can use other values for the number after `key=` as well. Any of the
addresses displayed by these commands are valid Solana wallet addresses. The
private portion associated with each address is stored securely on the Trezor device, and
is used to sign transactions from this address. Just make a note of which
keypair URL you used to derive any address you will be using to receive tokens.

If you are only planning to use a single address/keypair on your device, a good
easy-to-remember path might be to use the address at `key=0/<CHANGE>`. View this address
with:

```bash
solana-keygen pubkey usb://trezor?key=0/0
solana-keygen pubkey usb://trezor?key=0/1
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
[Explorer](https://explorer.solana.com/accounts) and paste the address in the
box to view the balance in your web browser.

Note: Any address with a balance of 0 SOL, such as a newly created one on your
Trezor, will show as "Not Found" in the explorer. Empty accounts and
non-existent accounts are treated the same in Solana. This will change when your
account address has some SOL in it.

### Send SOL from a Trezor

To send some tokens from an address controlled by your Trezor, you will need to
use the device to sign a transaction, using the same keypair URL you used to
derive the address. To do this, make sure your Trezor is plugged in.

The `solana transfer` command is used to specify to which address to send
tokens, how many tokens to send, and uses the `--keypair` argument to specify
which keypair is sending the tokens, which will sign the transaction, and the
balance from the associated address will decrease.

```bash
solana transfer RECIPIENT_ADDRESS AMOUNT --keypair KEYPAIR_URL_OF_SENDER
```

Below is a full example. First, an address is viewed at a certain keypair URL.
Second, the balance of that address is checked. Lastly, a transfer transaction
is entered to send `1` SOL to the recipient address
`7cvkjYAkUYs4W8XcXsca7cBrEGFeSUjeZmKoNBvEwyri`. When you hit Enter for a
transfer command, you will be prompted to approve the transaction details on
your Trezor device. Follow the prompts on the device and review the
transaction details. If they look correct, follow the prompts on your device.

```bash
~$ solana-keygen pubkey usb://trezor?key=0/0
CjeqzArkZt6xwdnZ9NZSf8D1CNJN1rjeFiyd8q7iLWAV

~$ solana balance CjeqzArkZt6xwdnZ9NZSf8D1CNJN1rjeFiyd8q7iLWAV
1.000005 SOL

~$ solana transfer 7cvkjYAkUYs4W8XcXsca7cBrEGFeSUjeZmKoNBvEwyri 1 --keypair usb://trezor?key=0/0
Waiting for your approval on Trezor hardware wallet
✅ Approved

Signature: kemu9jDEuPirKNRKiHan7ycybYsZp7pFefAdvWZRq5VRHCLgXTXaFVw3pfh87MQcWX4kQY4TjSBmESrwMApom1V
```

After approving the transaction on your device, the program will display the
transaction signature, and wait for the maximum number of confirmations (32)
before returning. This only takes a few seconds, and then the transaction is
finalized on the Solana network. You can view details of this or any other
transaction by going to the Transaction tab in the
[Explorer](https://explorer.solana.com/transactions) and paste in the
transaction signature.

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
solana-keygen pubkey usb://trezor\?key=0/0
```

## Support

You can find additional support and get help on the
[Solana StackExchange](https://solana.stackexchange.com).

Read more about [sending and receiving tokens](../../examples/transfer-tokens.md) and
[delegating stake](../../examples/delegate-stake.md). You can use your Ledger keypair
URL anywhere you see an option or argument that accepts a `<KEYPAIR>`.
