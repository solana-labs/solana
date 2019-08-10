## Staking a Validator
When your validator starts, it will have no stake, which means it will
ineligible to become leader.

Adding stake can be accomplished by using the `solana-wallet` CLI

Fire create a stake account keypair with `solana-keygen`:
```bash
$ solana-keygen new -o ~/validator-config/stake-keypair.json
```
and use the wallet's `delegate-stake` command to stake your validator with 42 lamports:
```bash
$ solana-wallet delegate-stake ~/validator-config/stake-keypair.json ~/validator-vote-keypair.json 42
```

Note that stake changes are applied at Epoch boundaries so it can take an hour
or more for the change to take effect.

Stake can be deactivate by running:
```bash
$ solana-wallet deactivate-stake ~/validator-config/stake-keypair.json ~/validator-vote-keypair.json
```
Note that a stake account may only be used once, so after deactivation, use the
wallet's `withdraw-stake` command to recover the previously staked lamports.
