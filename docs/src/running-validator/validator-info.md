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

The recommended dimensions for the validator icon are 360x360px and PNG format.

## Example Commands

Example publish command:

```bash
solana validator-info publish "Elvis Validator" -w "https://elvis-validates.com" -i "https://elvis-validates.com/my-icon.png"
```

Example query command:

```bash
solana validator-info get
```

which outputs

```text
Validator info from 8WdJvDz6obhADdxpGCiJKZsDYwTLNEDFizayqziDc9ah
  Validator pubkey: 6dMH3u76qZ7XG4bVboVRnBHR2FfrxEqTTTyj4xmyDMWo
  Info: {"iconUrl":"elvis","name":"Elvis Validator","website":"https://elvis-validates.com"}
```

For older accounts instead of `iconUrl` you might see `keybaseUsername` as those accounts used Keybase for their validator icon (see next section for further information).

## Keybase

Previously Keybase was used by validators to provide their validator icon, however Keybase has sunset its service and thus is no longer supported. Some old validator info accounts will still contain keybase usernames this is reflected in the serialized data returned when querying these validator info accounts.
