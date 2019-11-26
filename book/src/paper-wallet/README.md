# Paper Wallet

This document describes how to create and use a paper wallet with the Solana CLI tools.

## Overview

Solana provides a key generation tool to aid in creating and recovering keys with BIP39 compliant seed phrases. Solana CLI commands for running a validator  and staking tokens all support secure keypair input via seed phrases.

To learn more about the BIP39 standard, visit the Bitcoin BIPs Github repository [here](https://github.com/bitcoin/bips/blob/master/bip-0039.mediawiki).

## Installation

The tools used in this guide can all be installed following the [Installing the Validator Software](running-validator/validator-software.md) guide. After installation, you will have access to the `solana`, `solana-keygen`, and `solana-validator` tools.

{% page-ref page="paper-wallet/keypair.md" %}

{% page-ref page="paper-wallet/usage.md" %}
