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
can pass a *keypair URL* that uniquely identifies a keypair in a
hardware wallet.

## Supported Hardware Wallets

The Solana CLI supports the following hardware wallets:
 - [Ledger Nano S](ledger.md)

## Specify a Keypair URL

Solana defines a keypair URL format to uniquely locate any Solana keypair on a
hardware wallet connected to your computer.

The keypair URL has the following form, where square brackets denote optional
fields:

```text
usb://<MANUFACTURER>[/<WALLET_ID>][?key=<DERIVATION_PATH>]
```

`WALLET_ID` is a globally unique key used to disambiguate multiple devices.

`DERVIATION_PATH` is used to navigate to Solana keys within your hardware wallet.
The path has the form `<ACCOUNT>[/<CHANGE>]`, where each `ACCOUNT` and `CHANGE`
are positive integers.

For example, a fully qualified URL for a Ledger device might be:

```text
usb://ledger/BsNsvfXqQTtJnagwFWdBS7FBXgnsK8VZ5CmuznN85swK?key=0/0
```

All derivation paths implicitly include the prefix `44'/501'`, which indicates
the path follows the [BIP44 specifications](https://github.com/bitcoin/bips/blob/master/bip-0044.mediawiki)
and that any derived keys are Solana keys (Coin type 501).  The single quote
indicates a "hardened" derivation. Because Solana uses Ed25519 keypairs, all
derivations are hardened and therefore adding the quote is optional and
unnecessary.
