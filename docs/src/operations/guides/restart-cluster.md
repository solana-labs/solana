## Restarting a cluster

### Step 1. Identify the latest optimistically confirmed slot for the cluster

In Solana 1.14 or greater, run the following command to output the latest
optimistically confirmed slot your validator observed:
```bash
solana-ledger-tool -l ledger latest-optimistic-slots
```

In Solana 1.13 or less, the latest optimistically confirmed can be found by looking for the more recent occurrence of
[this](https://github.com/solana-labs/solana/blob/0264147d42d506fb888f5c4c021a998e231a3e74/core/src/optimistic_confirmation_verifier.rs#L71)
metrics datapoint.

Call this slot `SLOT_X`

Note that it's possible that some validators observed an optimistically
confirmed slot that's greater than others before the outage.  Survey the other
validators on the cluster to ensure that a greater optimistically confirmed slot
does not exist before proceeding. If a greater slot value is found use it
instead.


### Step 2. Stop the validator(s)

### Step 3. Optionally install the new solana version

### Step 4. Create a new snapshot for slot `SLOT_X` with a hard fork at slot `SLOT_X`

```bash
$ solana-ledger-tool -l <LEDGER_PATH> --snapshot-archive-path <SNAPSHOTS_PATH> --incremental-snapshot-archive-path <INCREMENTAL_SNAPSHOTS_PATH> create-snapshot SLOT_X <SNAPSHOTS_PATH> --hard-fork SLOT_X
```

The snapshots directory should now contain the new snapshot.
`solana-ledger-tool create-snapshot` will also output the new shred version, and bank hash value,
call this NEW_SHRED_VERSION and NEW_BANK_HASH respectively.

Adjust your validator's arguments:

```bash
 --wait-for-supermajority SLOT_X
 --expected-bank-hash NEW_BANK_HASH
```

Then restart the validator.

Confirm with the log that the validator booted and is now in a holding pattern at `SLOT_X`, waiting for a super majority.

Once NEW_SHRED_VERSION is determined, nudge foundation entrypoint operators to update entrypoints.

### Step 5. Announce the restart on Discord:

Post something like the following to #announcements (adjusting the text as appropriate):

> Hi @Validators,
>
> We've released v1.1.12 and are ready to get testnet back up again.
>
> Steps:
>
> 1. Install the v1.1.12 release: https://github.com/solana-labs/solana/releases/tag/v1.1.12
> 2. a. Preferred method, start from your local ledger with:
>
> ```bash
> solana-validator
>   --wait-for-supermajority SLOT_X     # <-- NEW! IMPORTANT! REMOVE AFTER THIS RESTART
>   --expected-bank-hash NEW_BANK_HASH  # <-- NEW! IMPORTANT! REMOVE AFTER THIS RESTART
>   --hard-fork SLOT_X                  # <-- NEW! IMPORTANT! REMOVE AFTER THIS RESTART
>   --no-snapshot-fetch                 # <-- NEW! IMPORTANT! REMOVE AFTER THIS RESTART
>   --entrypoint entrypoint.testnet.solana.com:8001
>   --known-validator 5D1fNXzvv5NjV1ysLjirC4WY92RNsVH18vjmcszZd8on
>   --expected-genesis-hash 4uhcVJyU9pJkvQyS88uRDiswHXSCkY3zQawwpjk2NsNY
>   --only-known-rpc
>   --limit-ledger-size
>   ...                                # <-- your other --identity/--vote-account/etc arguments
> ```
>
> b. If your validator doesn't have ledger up to slot SLOT_X or if you have deleted your ledger, have it instead download a snapshot with:
>
> ```bash
> solana-validator
>   --wait-for-supermajority SLOT_X     # <-- NEW! IMPORTANT! REMOVE AFTER THIS RESTART
>   --expected-bank-hash NEW_BANK_HASH  # <-- NEW! IMPORTANT! REMOVE AFTER THIS RESTART
>   --entrypoint entrypoint.testnet.solana.com:8001
>   --known-validator 5D1fNXzvv5NjV1ysLjirC4WY92RNsVH18vjmcszZd8on
>   --expected-genesis-hash 4uhcVJyU9pJkvQyS88uRDiswHXSCkY3zQawwpjk2NsNY
>   --only-known-rpc
>   --limit-ledger-size
>   ...                                # <-- your other --identity/--vote-account/etc arguments
> ```
>
>      You can check for which slots your ledger has with: `solana-ledger-tool -l path/to/ledger bounds`
>
> 3. Wait until 80% of the stake comes online
>
> To confirm your restarted validator is correctly waiting for the 80%:
> a. Look for `N% of active stake visible in gossip` log messages
> b. Ask it over RPC what slot it's on: `solana --url http://127.0.0.1:8899 slot`. It should return `SLOT_X` until we get to 80% stake
>
> Thanks!

### Step 7. Wait and listen

Monitor the validators as they restart. Answer questions, help folks,

## Troubleshooting

### 80% of the stake didn't participate in the restart, now what?
If less than 80% of the stake join the restart after a reasonable amount of
time, it will be necessary to retry the restart attempt with the stake from the
non-responsive validators removed.

The community should identify and come to social consensus on the set of
non-responsive validators. Then all participating validators return to Step 4
and create a new snapshot with additional `--destake-vote-account <PUBKEY>`
arguments for each of the non-responsive validator's vote account address

```bash
$ solana-ledger-tool -l ledger create-snapshot SLOT_X ledger --hard-fork SLOT_X \
    --destake-vote-account <VOTE_ACCOUNT_1> \
    --destake-vote-account <VOTE_ACCOUNT_2> \
    .
    .
    --destake-vote-account <VOTE_ACCOUNT_N> \
```

This will cause all stake associated with the non-responsive validators to be
immediately deactivated. All their stakers will need to re-delegate their stake
once the cluster restart is successful.
