# Paper Wallet

This document describes how to create and use a paper wallet with the Solana CLI
tools.

{% hint style="info" %}
We do not intend to advise on how to *securely* create or manage paper wallets.
Please research the security concerns carefully.
{% endhint %}

## Overview

Solana provides a key generation tool to derive keys from BIP39 compliant seed
phrases. Solana CLI commands for running a validator and staking tokens all
support keypair input via seed phrases.

To learn more about the BIP39 standard, visit the Bitcoin BIPs Github repository
[here](https://github.com/bitcoin/bips/blob/master/bip-0039.mediawiki).

## Installation
Solana provides a CLI tool for key generation called `solana-keygen`

### Via Rust Cargo
First, install Rust's package manager `cargo`

```bash
$ curl https://sh.rustup.rs -sSf | sh
$ source $HOME/.cargo/env
```

Then, install the `solana-keygen` tool

```bash
cargo install solana-keygen
```

### Via Tarball
First download the desired release tarball from GitHub. The examples below will
retrieve the most recent release. If you would like to download a specific
version instead replace `latest/download` with `download/VERSION` where VERSION
is a tag name from https://github.com/solana-labs/solana/releases (ie. v0.21.0).

MacOS
```bash
$ curl -L -sSf -o solana-release.tar.bz2 'https://github.com/solana-labs/solana/releases/latest/download/solana-release-x86_64-apple-darwin.tar.bz2'
```

Linux
```bash
$ curl -L -sSf -o solana-release.tar.bz2 'https://github.com/solana-labs/solana/releases/latest/download/solana-release-x86_64-unknown-linux-gnu.tar.bz2'
```

Next, extract the tarball
```bash
$ tar xf solana-release.tar.bz2
```

Finally, `solana-keygen` can be run by
```bash
$ solana-release/bin/solana-keygen
```

If you would like to follow the remainder of these instructions without typing
the leading path to `solana-keygen`, add it to your PATH environment variable
with the following command
```bash
$ export PATH="$(pwd)/solana-release/bin:${PATH}"
```
This can be made permanent by adding it to your `~/.profile`

{% page-ref page="paper-wallet/keypair.md" %}

{% page-ref page="paper-wallet/usage.md" %}
