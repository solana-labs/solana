---
title: Publishing Validator Info
---

You can publish your validator information to the chain to be publicly visible to other users.

## Run solana validator-info

Run the solana CLI to populate a validator info account:

```bash
solana validator-info publish --keypair ~/validator-keypair.json <VALIDATOR_INFO_ARGS> <VALIDATOR_NAME>
```

For details about optional fields for VALIDATOR_INFO_ARGS:

```bash
solana validator-info publish --help
```

## Example Commands

Example publish command:

```bash
solana validator-info publish "Elvis Validator" -n elvis -w "https://elvis-validates.com"
```

Example query command:

```bash
solana validator-info get
```

which outputs

```text
Validator info from 8WdJvDz6obhADdxpGCiJKZsDYwTLNEDFizayqziDc9ah
  Validator pubkey: 6dMH3u76qZ7XG4bVboVRnBHR2FfrxEqTTTyj4xmyDMWo
  Info: {"githubUsername":"elvis","name":"Elvis Validator","website":"https://elvis-validates.com"}
```

## Github

Including a Github username allows client applications \(like the Solana
Network Explorer\) to automatically pull in your validator public profile,
including cryptographic proofs, brand identity, etc. To connect your validator
pubkey with Github:

1. Create a public repository on github.com with the naming convention `solana-validator-<PUBKEY>` - replace `<PUBKEY>` with your validator's identity public key.
2. Ensure this repository contains a PNG image in the root directory named `validator-image-<PUBKEY>.png` on the branch named `main`
3. Add or update your `solana validator-info` with your Github username. The

   CLI will verify the `solana-validator-<PUBKEY>` repository exists

## Keybase

Previously Keybase was used by validators in place of Github, however Keybase has sunset its service and thus is no longer supported. Some old validator info accounts will still contain keybase usernames this is reflected in the serialized data returend when querying these validator info accounts.
