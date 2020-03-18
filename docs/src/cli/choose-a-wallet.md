# Choose a Wallet

Keypairs are stored in *wallets* and wallets come in many forms. A wallet might
be a directory in your computer's file system, a piece of paper, or a
specialized device called a *hardware wallet*. Some wallets are easier to use
than others.  Some are more secure than others. In this section, we'll compare
and contrast different types of wallets and help you choose the wallet that
best fits your needs.

## File System Wallet

A *file system wallet*, aka an FS wallet, is a directory in your computer's
file system. Each file in the directory holds a keypair.

### FS Wallet Security

An FS wallet is the most convenient and least secure form of wallet. It is
convenient because the keypair is stored in a simple file. You can generate as
many keys as you'd like and trivially back them up by copying the files. It is
insecure because the keypair files are **unencrypted**. If you are the only
user of your computer and you are confident it is free of malware, an FS wallet
is a fine solution for small amounts of cryptocurrency. If, however, your
computer contains malware and is connected to the Internet, that malware may
upload your keys and use them to take your tokens. Likewise, because the
keypairs are stored on your computer as files, a skilled hacker with physical
access to your computer may be able to access it. Using an encrypted hard
drive, such as FileVault on MacOS, minimizes that risk.

## Paper Wallet

A *paper wallet* is a collection of *seed phrases* written on paper. A seed
phrase is some number of words (typically 12 or 24) that can be used to
regenerate a keypair on demand.

### Paper Wallet Security

In terms of convenience versus security, a paper wallet sits at the opposite
side of the spectrum from an FS wallet. It is terribly inconvenient to use, but
offers excellent security. That high security is further amplified when paper
wallets are used in conjunction with [offline
signing](../offline-signing/index.md). Custody services such as [Coinbase
Custody](https://custody.coinbase.com/) use this combination.  Paper wallets
and custody services are an excellent way to secure a large number of tokens
for a long period of time.

## Hardware Wallet

A hardware wallet is a small handheld device that stores keypairs and provides
some interface for signing transactions.

### Hardware Wallet Security

A hardware wallet, such as the [Ledger hardware
wallet](https://www.ledger.com/), offers a great blend of security and
convenience for cryptocurrencies. It effectively automates the process of
offline signing while retaining nearly all the convenience of an FS wallet.
In contrast to offline signing with paper wallets, which we regard as highly
secure, offline signing with hardware wallets presents some security concerns:

* Keys could be leaked by a malicious app on the hardware wallet
* Keys could be stolen by a malicious client via a bug in the hardware wallet's
  operating system
* Keys could be read from storage if hardware wallet is lost or stolen

To keep your hardware wallet tokens safe, we suggest:

* Only install apps using the vendor's app manager
* Keep the hardware wallet's firmware up to date
* Put keys for large amounts of tokens into their own hardware wallets. Store
  in a secure location.

## Which Wallet is Best?

Different people will have different needs, but if you are still unsure what's
best for you after reading the comparisons above, go with a [Ledger Nano
S](https://shop.ledger.com/products/ledger-nano-s). The [Nano S is
well-integrated into Solana's tool suite](../remote-wallet/ledger) and offers
an outstanding blend of security and convenience.
