# Publishing Validator Info

You can publish your validator information to the chain to be publicly visible
to other users.

## Run solana validator-info
Run the solana CLI to populate a validator-info account:
```bash
$ solana validator-info publish ~/validator-keypair.json <VALIDATOR_NAME> <VALIDATOR_INFO_ARGS>
```
Optional fields for VALIDATOR_INFO_ARGS:
* Website
* Keybase Username
* Details

## Keybase

Including a Keybase username allows client applications (like the Solana Network
Explorer) to automatically pull in your validator public profile, including
cryptographic proofs, brand identity, etc. To connect your validator pubkey with
Keybase:

1. Join https://keybase.io/ and complete the profile for your validator
2. Add your validator **identity pubkey** to Keybase:
  * Create an empty file on your local computer called `validator-<PUBKEY>`
  * In Keybase, navigate to the Files section, and upload your pubkey file to
  a `solana` subdirectory in your public folder: `/keybase/public/<KEYBASE_USERNAME>/solana`
  * To check your pubkey, ensure you can successfully browse to
  `https://keybase.pub/<KEYBASE_USERNAME>/solana/validator-<PUBKEY>`
3. Add or update your `solana validator-info` with your Keybase username. The
CLI will verify the `validator-<PUBKEY>` file
