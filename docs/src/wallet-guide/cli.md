---
title: Command Line Wallets
---

Solana supports several different types of wallets that can be used to interface
directly with the Solana command-line tools.

To use a Command Line Wallet, you must first [install the Solana CLI tools](../cli/install-solana-cli-tools.md)

## File System Wallet

A _file system wallet_, aka an FS wallet, is a directory in your computer's
file system. Each file in the directory holds a keypair.

### File System Wallet Security

A file system wallet is the most convenient and least secure form of wallet. It
is convenient because the keypair is stored in a simple file. You can generate as
many keys as you would like and trivially back them up by copying the files. It
is insecure because the keypair files are **unencrypted**. If you are the only
user of your computer and you are confident it is free of malware, an FS wallet
is a fine solution for small amounts of cryptocurrency. If, however, your
computer contains malware and is connected to the Internet, that malware may
upload your keys and use them to take your tokens. Likewise, because the
keypairs are stored on your computer as files, a skilled hacker with physical
access to your computer may be able to access it. Using an encrypted hard
drive, such as FileVault on MacOS, minimizes that risk.

[File System Wallet](file-system-wallet.md)

## Paper Wallet

A _paper wallet_ is a collection of _seed phrases_ written on paper. A seed
phrase is some number of words (typically 12 or 24) that can be used to
regenerate a keypair on demand.

### Paper Wallet Security

In terms of convenience versus security, a paper wallet sits at the opposite
side of the spectrum from an FS wallet. It is terribly inconvenient to use, but
offers excellent security. That high security is further amplified when paper
wallets are used in conjunction with [offline signing](../offline-signing.md).

[Paper Wallets](paper-wallet.md)

## Hardware Wallet

A hardware wallet is a small handheld device that stores keypairs and provides
some interface for signing transactions.

### Hardware Wallet Security

A hardware wallet, such as the
[Ledger hardware wallet](https://www.ledger.com/), offers a great blend of
security and convenience for cryptocurrencies. It effectively automates the
process of offline signing while retaining nearly all the convenience of a file
system wallet.

[Hardware Wallets](hardware-wallets.md)
