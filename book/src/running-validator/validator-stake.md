# Staking

**By default your validator will have no stake.** This means it will be
ineligible to become leader.

## Monitoring Catch Up

To delegate stake, first make sure your validator is running and has caught up
to the cluster. It may take some time to catch up after your validator boots.
Use the `catchup` command to monitor your validator through this process:

```bash
solana catchup ~/validator-keypair.json
```

Until your validator has caught up, it will not be able to vote successfully and
stake cannot be delegated to it.

Also if you find the cluster's slot advancing faster than yours, you will likely
never catch up. This typically implies some kind of networking issue between
your validator and the rest of the cluster.

## Create Stake Keypair

If you haven’t already done so, create a staking keypair. If you have completed
this step, you should see the “validator-stake-keypair.json” in your Solana
runtime directory.

```bash
solana-keygen new -o ~/validator-stake-keypair.json
```

## Delegate Stake

Now delegate 1 SOL to your validator by first creating your stake account:

```bash
solana create-stake-account ~/validator-stake-keypair.json 1 SOL
```

and then delegating that stake to your validator:

```bash
solana delegate-stake ~/validator-stake-keypair.json ~/validator-vote-keypair.json
```

> Don’t delegate your remaining SOL, as your validator will use those tokens to vote.

Stakes can be re-delegated to another node at any time with the same command,
but only one re-delegation is permitted per epoch:

```bash
solana delegate-stake ~/validator-stake-keypair.json ~/some-other-validator-vote-keypair.json
```

Assuming the node is voting, now you're up and running and generating validator
rewards. Rewards are paid automatically on epoch boundaries.

The rewards lamports earned are split between your stake account and the vote
account according to the commission rate set in the vote account. Rewards can
only be earned while the validator is up and running. Further, once staked, the
validator becomes an important part of the network. In order to safely remove a
validator from the network, first deactivate its stake.

At the end of each slot, a validator is expected to send a vote transaction.
These vote transactions are paid for by lamports from a validator's identity
account.

This is a normal transaction so the standard transaction fee will apply. The
transaction fee range is defined by the genesis block. The actual fee will
fluctuate based on transaction load. You can determine the current fee via the
[RPC API “getRecentBlockhash”](../api-reference/jsonrpc-api.md#getrecentblockhash)
before submitting a transaction.

Learn more about [transaction fees here](../implemented-proposals/transaction-fees.md).

## Validator Stake Warm-up

Stakes need to warm up, and warmup increments are applied at Epoch boundaries,
so it can take an hour or more for stake to come fully online.

To monitor your validator during its warmup period:

* View your vote account:`solana vote-account ~/validator-vote-keypair.json` This displays the current state of all the votes the validator has submitted to the network.
* View your stake account, the delegation preference and details of your stake:`solana stake-account ~/validator-stake-keypair.json`
* `solana uptime ~/validator-vote-keypair.json` will display the voting history \(aka, uptime\) of your validator over recent Epochs
* `solana validators` displays the current active stake of all validators, including yours
* `solana stake-history ` shows the history of stake warming up and cooling down over recent epochs
* Look for log messages on your validator indicating your next leader slot: `[2019-09-27T20:16:00.319721164Z INFO solana_core::replay_stage] <VALIDATOR_IDENTITY_PUBKEY> voted and reset PoH at tick height ####. My next leader slot is ####`
* Once your stake is warmed up, you will see a stake balance listed for your validator on the [Solana Network Explorer](http://explorer.solana.com/validators)

## Monitor Your Staked Validator

Confirm your validator becomes a [leader](../terminology.md#leader)

* After your validator is caught up, use the `solana balance` command to monitor the earnings as your validator is selected as leader and collects transaction fees
* Solana nodes offer a number of useful JSON-RPC methods to return information about the network and your validator's participation. Make a request by using curl \(or another http client of your choosing\), specifying the desired method in JSON-RPC-formatted data. For example:

```bash
  // Request
  curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc":"2.0","id":1, "method":"getEpochInfo"}' http://localhost:8899

  // Result
  {"jsonrpc":"2.0","result":{"epoch":3,"slotIndex":126,"slotsInEpoch":256},"id":1}
```

Helpful JSON-RPC methods:

* `getEpochInfo`[An epoch](../terminology.md#epoch) is the time, i.e. number of [slots](../terminology.md#slot), for which a [leader schedule](../terminology.md#leader-schedule) is valid. This will tell you what the current epoch is and how far into it the cluster is.
* `getVoteAccounts` This will tell you how much active stake your validator currently has. A % of the validator's stake is activated on an epoch boundary. You can learn more about staking on Solana [here](../cluster/stake-delegation-and-rewards.md).
* `getLeaderSchedule` At any given moment, the network expects only one validator to produce ledger entries. The [validator currently selected to produce ledger entries](../cluster/leader-rotation.md#leader-rotation) is called the “leader”. This will return the complete leader schedule \(on a slot-by-slot basis\) for currently activated stake, the identity pubkey will show up 1 or more times here.

## Deactivating Stake

Before detaching your validator from the cluster, you should deactivate the
stake that was previously delegated by running:

```bash
solana deactivate-stake ~/validator-stake-keypair.json
```

Stake is not deactivated immediately and instead cools down in a similar fashion
as stake warm up.  Your validator should remain attached to the cluster while
the stake is cooling down. While cooling down, your stake will continue to earn
rewards. Only after stake cooldown is it safe to turn off your validator or
withdraw it from the network. Cooldown may take several epochs to complete,
depending on active stake and the size of your stake.

Note that a stake account may only be used once, so after deactivation, use the
cli's `withdraw-stake` command to recover the previously staked lamports.
