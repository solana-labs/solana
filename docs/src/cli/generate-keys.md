# Generate Keys

To receive tokens, you will need an address for others to send tokens to. In
Solana, an address is the public key of a *keypair*. There are a variety
of techniques for generating keypairs. The method you choose will depend on how
you choose to store keypairs.  Keypairs are stored in *wallets*. A wallet might
be a directory on your computer's file system, a piece of paper, or a specialized
device called a *hardware wallet*. Some wallets are easier to use than others.
Some are more secure than others.  We'll go over each type of wallet here and dive
into the various tradeoffs.

## File System Wallet

A *file system wallet* (aka FS wallet) is a directory in your computer's file system.
Each file in the directory holds a keypair.

### FS Wallet Security

An FS wallet is the most convenient and least secure form of wallets. It is
convenient because the keypair is stored in a simple file. You can generate as
many keys as you'd like and trivially make copies to back them up. If you
are confident your computer has no malware and that no other people will use
it, an FS wallet is a fine solution for small amounts of SOL. If however,
your computer contains malware and is connected to the Internet, that malware may
upload your keys and used to take your tokens. Likewise, because the keypair is
stored on your computer in a simple file, a skilled hacker with physical access
to your computer may be able to access it. Using an encrypted hard drive, such
as FileVault on MacOS, minimizes that risk.

### Generate an FS Wallet Key

Use Solana's command-line tool `solana-keygen` to generate keypair files. For
example, run the following from a command-line shell:

```bash
mkdir ~/my-solana-wallet
solana-keygen new -o ~/my-solana-wallet/my-keypair.json
```

If you view the file, you will a long list of numbers, such as:

```text
[42,200,155,187,52,228,32,9,179,129,192,196,149,41,177,47,87,228,5,19,70,82,170,6,142,114,68,85,124,34,165,216,110,186,177,254,198,143,235,59,173,59,17,250,142,32,66,162,130,62,53,252,48,33,148,38,149,17,81,154,95,178,163,164]
```

This file contains your **unencrypted** keypair. In fact, even if you specify
a password, that password applies to the recovery seed phrase, not the file. Do
not share this file with others. Anyone with access to this file will have access
to all tokens sent to its public key. Instead, you should share only its public
key. To display its public key, run:

```bash
solana-keygen pubkey ~/my-solana-wallet/my-keypair.json
```

It will output a string of characters, such as:

```text
ErRr1caKzK8L8nn4xmEWtimYRiTCAZXjBtVphuZ5vMKy
```

This is the public key corresponding to the keypair in `~/my-solana-wallet/my-keypair.json`.

## Paper Wallet

A *paper wallet* is a collection of *seed phrases* written on paper. A seed phrase
is some number of words (typically 12 or 24) that can be used to regenerate a
keypair on demand.

### Paper Wallet Security

In terms of convenience versus security, a paper wallet sits at the opposite
side of the spectrum. It is is terribly inconvenient to use, but offers
excellent security. That high security is further amplified when paper wallets
are used in combination with offline signing. Custody services such as Coinbase
Custody use this combination of techniques. Paper wallets and custody services
are a great way to secure a large number of tokens for a long period of time.

### Generate a Paper Wallet Key

See [Paper Wallets](paper_wallet.md).

## Hardware Wallet

A hardware wallet is a small handheld device that stores private keys
and provides some interface for signing transactions.

### Hardware Wallet Security

A hardware wallet, such as the Ledger hardware wallet, offers a great blend of
security and convenience for cryptocurrencies. It effectively automates the
process of offline transaction signing while retaining nearly all the
convenience of hot keys. Some security concerns with hardware wallets:

* Keys leaked by a malicious app on the hardware wallet
* Keys stolen by a malicious client via a bug in the hardware wallet's operating system
* Keys read from storage if hardware wallet itself is lost or stolen

To keep your wallet tokens secure, we suggest:

* Only install apps using the vendor's app manager
* Keep the hardware wallet's firmware up-to-date
* Put keys for large amounts of tokens into its own hardware wallet. Store it
  in a secure location.

### Generate a Hardware Wallet Key

See [Hardware Wallets](../remote-wallet).
