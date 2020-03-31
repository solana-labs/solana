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

## Specify a Hardware Wallet Key

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

All derivation paths implicitly include the prefix `44'/501'`, which indicates
the path follows the [BIP44 specifications](https://github.com/bitcoin/bips/blob/master/bip-0044.mediawiki)
and that any derived keys are Solana keys (Coin type 501).  The single quote
indicates a "hardened" derivation. Because Solana uses Ed25519 keypairs, all
derivations are hardened and therefore adding the quote is optional and
unnecessary.

For example, a fully qualified URL for a Ledger device might be:

```text
usb://ledger/BsNsvfXqQTtJnagwFWdBS7FBXgnsK8VZ5CmuznN85swK?key=0/0
```

## Manage Multiple Hardware Wallets

It is sometimes useful to sign a transaction with keys from multiple hardware
wallets. Signing with multiple wallets requires *fully qualified keypair URLs*.
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
usb://ledger/BsNsvfXqQTtJnagwFWdBS7FBXgnsK8VZ5CmuznN85swK?key=0/0
```

but where `BsNsvfXqQTtJnagwFWdBS7FBXgnsK8VZ5CmuznN85swK` is your `WALLET_ID`.

With your fully qualified URL, you can connect multiple hardware wallets to
the same computer and uniquely identify a keypair from any of them.

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
character, you can disable it explictly with a backslash in your keypair URLs.
For example:

```bash
solana-keygen pubkey usb://ledger\?key=0
```
