## Staking a Validator
When your validator starts, it will have no stake, which means it will
ineligible to become leader.

Adding stake can be accomplished by using the `solana-wallet` CLI.  First
obtain the public key for your validator's vote account with:
```bash
$ solana-keygen pubkey ~/validator-config/vote-keypair.json
```
This will output a base58-encoded value that looks similar to
`DhUYZR98qFLLrnHg2HWeGhBQJ9tru7nwdEfYm8L8HdR9`. Then create a stake account
keypair with `solana-keygen`:
```bash
$ solana-keygen new -o ~/validator-config/stake-keypair.json
```
and use the wallet's `delegate-stake` command to stake your validator with 42 lamports:
```bash
$ solana-wallet delegate-stake ~/validator-config/stake-keypair.json [VOTE PUBKEY] 42
```

Note that stake changes are applied at Epoch boundaries so it can take an hour
or more for the change to take effect.

Stake can be deactivate by running:
```bash
$ solana-wallet deactivate-stake ~/validator-config/stake-keypair.json [VOTE PUBKEY]
```
Note that a stake account may only be used once, so after deactivation, use the
wallet's `withdraw-stake` command to recover the previously staked lamports.
