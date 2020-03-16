# Hardware Wallets

Signing a transaction requires a private key, but storing a private
key on your personal computer or phone leaves it subject to theft.
Adding a password to your key adds security, but many people prefer
to take it a step further and move their private keys to a separate
physical device called a *hardware wallet*. A hardware wallet is a
small handheld device that stores private keys and provides some
interface for signing transactions.

The Solana CLI has first class support for hardware wallets. Anywhere
you use a keypair filepath (denoted as `<KEYPAIR>` in usage docs), you
can pass a URL that uniquely identifies a keypair in a hardware
wallet.

## Supported Hardware Wallets

The Solana CLI supports the following hardware wallets:
- Ledger Nano S {% page-ref page="ledger.md" %}

## Specify A Hardware Wallet Key

Solana defines a format for the URL protocol "usb://" to uniquely locate any
Solana key on any hardware wallet connected to your computer.

The URL has the following form, where square brackets denote optional fields:

```text
usb://<MANUFACTURER>[/<PRODUCT>[/<WALLET_KEY>]][?key=<DERIVATION_PATH>]
```

`PRODUCT` is optional and defaults to an auto-detected value. When using a Ledger
Nano S, for example, it defaults to "nano-s" without quotes.

`WALLET_KEY` is used to disambiguate multiple devices. Each device has a unique
master key and from that key derives a separate unique key per app.

`DERVIATION_PATH` is used to navigate to Solana keys within your hardware wallet.
The path has the form `<ACCOUNT>[/<CHANGE>]`, where each `ACCOUNT` and `CHANGE`
are positive integers.

All derivation paths implicitly include the prefix `44'/501'`, which indicates
the path follows the [BIP44 specifications](https://github.com/bitcoin/bips/blob/master/bip-0044.mediawiki)
and that any derived keys are Solana keys (Coin type 501).  The single quote
indicates a "hardened" derivation. Because Solana uses Ed25519 keypairs, all
derivations are hardened and therefore adding the quote is optional and
unnecessary.

For example, a fully qualified URL for a Ledger device might be:

```text
usb://ledger/nano-s/BsNsvfXqQTtJnagwFWdBS7FBXgnsK8VZ5CmuznN85swK?key=0/0
```

## Manage Multiple Hardware Wallets

It is sometimes useful to sign a transaction with keys from multiple hardware
wallets. Signing with multiple wallets requires *fully qualified USB URLs*.
When the URL is not fully qualified, the Solana CLI will prompt you with
the fully qualified URLs of all connected hardware wallets, and ask you to
choose which wallet to use for each signature.

Instead of using the interactive prompts, you can generate fully qualified
URLs using the Solana CLI `resolve-signer` command. For example, try
connecting a Ledger Nano-S to USB, unlock it with your pin, and running the
following command:

```text
solana resolve-signer usb://ledger?key=0/0
```

You will see output similar to:

```text
usb://ledger/nano-s/BsNsvfXqQTtJnagwFWdBS7FBXgnsK8VZ5CmuznN85swK?key=0/0
```

but where `BsNsvfXqQTtJnagwFWdBS7FBXgnsK8VZ5CmuznN85swK` is your `WALLET_KEY`.

With your fully qualified URL, you can connect multiple hardware wallets to
the same computer and uniquely identify a keypair from any of them.
