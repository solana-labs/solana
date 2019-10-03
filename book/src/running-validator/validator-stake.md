# Staking

When your validator starts, it will have no stake, which means it will be ineligible to become leader.

Adding stake can be accomplished by using the `solana` CLI

First create a stake account keypair with `solana-keygen`:

```bash
$ solana-keygen new -o ~/validator-config/stake-keypair.json
```

and use the cli's `create-stake-account` and `delegate-stake` commands to stake your validator with 42 lamports:

```bash
$ solana create-stake-account ~/validator-config/stake-keypair.json 42 lamports
$ solana delegate-stake ~/validator-config/stake-keypair.json ~/validator-vote-keypair.json
```

Note that stakes need to warm up, and warmup increments are applied at Epoch boundaries, so it can take an hour or more for the change to fully take effect.

Stakes can be re-delegated to another node at any time with the same command:

```bash
$ solana delegate-stake ~/validator-config/stake-keypair.json ~/some-other-validator-vote-keypair.json
```

Assuming the node is voting, now you're up and running and generating validator rewards. You'll want to periodically redeem/claim your rewards:

```bash
$ solana redeem-vote-credits ~/validator-config/stake-keypair.json ~/validator-vote-keypair.json
```

The rewards lamports earned are split between your stake account and the vote account according to the commission rate set in the vote account. Rewards can only be earned while the validator is up and running. Further, once staked, the validator becomes an important part of the network. In order to safely remove a validator from the network, first deactivate its stake.

Stake can be deactivated by running:

```bash
$ solana deactivate-stake ~/validator-config/stake-keypair.json ~/validator-vote-keypair.json
```

The stake will cool down, deactivate over time. While cooling down, your stake will continue to earn rewards. Only after stake cooldown is it safe to turn off your validator or withdraw it from the network. Cooldown may take several epochs to complete, depending on active stake and the size of your stake.

Note that a stake account may only be used once, so after deactivation, use the cli's `withdraw-stake` command to recover the previously staked lamports.

Be sure and redeem your credits before withdrawing all your lamports. Once the account is fully withdrawn, the account is destroyed.

