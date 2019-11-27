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

The tools used in this guide can all be installed by following the
[Installing the Validator Software](running-validator/validator-software.md)
guide. After installation, you will have access to the `solana`,
`solana-keygen`, and `solana-validator` tools.

{% page-ref page="paper-wallet/keypair.md" %}

{% page-ref page="paper-wallet/usage.md" %}
