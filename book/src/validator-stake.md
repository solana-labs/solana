## Staking a Validator
When your validator starts, it will have no stake, which means it will be
ineligible to become leader.

Adding stake can be accomplished by using the `solana` CLI

First create a stake account keypair with `solana-keygen`:
```bash
$ solana-keygen new -o ~/validator-config/stake-keypair.json
```
and use the cli's `delegate-stake` command to stake your validator with 42 lamports:
```bash
$ solana delegate-stake ~/validator-config/stake-keypair.json ~/validator-vote-keypair.json 42 lamports
```

Note that stakes need to warm up, and warmup increments are applied at Epoch boundaries, so it can take an hour
or more for the change to fully take effect.

Assuming your node is voting, now you're up and running and generating validator rewards.  You'll want
to periodically redeem/claim your rewards:

```bash
$ solana redeem-vote-credits ~/validator-config/stake-keypair.json ~/validator-vote-keypair.json
```

The rewards lamports earned are split between your stake account and the vote account according to the
commission rate set in the vote account.

Stake can be deactivated by running:
```bash
$ solana deactivate-stake ~/validator-config/stake-keypair.json ~/validator-vote-keypair.json
```

The stake will cool down, deactivate over time.  While cooling down, your stake will continue to earn
rewards.

Note that a stake account may only be used once, so after deactivation, use the
cli's `withdraw-stake` command to recover the previously staked lamports.

Be sure and redeem your credits before withdrawing all your lamports.
Once the account is fully withdrawn, the account is destroyed.
