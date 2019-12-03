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

First, install Rust's package manager `cargo`

```bash
$ curl https://sh.rustup.rs -sSf | sh
$ source $HOME/.cargo/env
```

Then, install the `solana-keygen` tool

```bash
cargo install solana-keygen
```

{% page-ref page="paper-wallet/keypair.md" %}

{% page-ref page="paper-wallet/usage.md" %}
