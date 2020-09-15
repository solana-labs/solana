## Restarting a cluster

### Step 1. Identify the slot that the cluster will be restarted at

This will probably be the last root that was made. Call this slot `SLOT_X`

### Step 2. Stop the validator(s)

### Step 3. Install the new solana version

### Step 4. Create a new snapshot for slot `SLOT_X` with a hard fork at slot `SLOT_X`

```bash
$ solana-ledger-tool -l ledger create-snapshot SLOT_X ledger --hard-fork SLOT_X
```

The ledger directory should now contain the new snapshot.
`solana-ledger-tool create-snapshot` will also output the new shred version, and bank hash value,
call this NEW\_SHRED\_VERSION and NEW\_BANK\_HASH respectively.

Adjust your validator's arugments:

```bash
 --wait-for-supermajority SLOT_X
 --expected-bank-hash NEW_BANK_HASH
```

Then restart the validator.

Confirm with the log that the validator booted and is now in a holding pattern at `SLOT_X`, waiting for a super majority.

### Step 5. Update shred documentation

Edit `https://github.com/solana-labs/solana/blob/master/docs/src/clusters.md`,
replacing the old shred version with NEW\_SHRED\_VERSION. Ensure the edits make it into the release channel

### Step 6. Announce the restart on Discord:

Post something like the following to #announcements (adjusting the text as appropriate):

> Hi @Validators,
>
> We've released v1.1.12 and are ready to get TdS back up again.
>
> Steps:
> 1. Install the v1.1.12 release: https://github.com/solana-labs/solana/releases/tag/v1.1.12
> 2.
>   a. Preferred method, start from your local ledger with:
>
> ```bash
> solana-validator
>   --wait-for-supermajority 12961040  # <-- NEW! IMPORTANT!
>   --expected-bank-hash 6q2oTgs8FiJxra2Zo1N8tzWqo5b6uGbjmFgoWDsXxchY    # <-- NEW! IMPORTANT!
>   --hard-fork 56096                  # <-- NEW! IMPORTANT!
>   --entrypoint 35.203.170.30:8001    # <-- Same as before
>   --trusted-validator 5D1fNXzvv5NjV1ysLjirC4WY92RNsVH18vjmcszZd8on
>   --expected-genesis-hash 4uhcVJyU9pJkvQyS88uRDiswHXSCkY3zQawwpjk2NsNY
>   --no-untrusted-rpc
>   --limit-ledger-size
>   --no-snapshot-fetch
>   --no-genesis-fetch
>   ...                                # <-- your other --identity/--vote-account/etc arguments
> ```
>   b. If your validator doesn't have ledger up to slot 21042873, have it download a snapshot by removing
>      `--no-snapshot-fetch --no-genesis-fetch --hard-fork 21042873` arguments.
>      You can check for which slots your ledger has with: `solana-ledger-tool -l path/to/ledger bounds`
>
> 3. Wait until 80% of the stake comes online
>
> To confirm your restarted validator is correctly waiting for the 80%:
> a. Look for `N% of active stake visible in gossip` log messages
> b. Ask it over RPC what slot it's on: `solana --url http://127.0.0.1:8899 slot`.  It should return `12961040` until we get to 80% stake
>
> Thanks!

### Step 7. Wait and listen

Monitor the validators as they restart. Answer questions, help folks,
