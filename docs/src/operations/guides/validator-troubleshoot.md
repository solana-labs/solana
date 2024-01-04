---
title: Troubleshooting
---

There is a `#validator-support` Discord channel available to reach other
testnet participants, [https://solana.com/discord](https://solana.com/discord)

## Useful Links & Discussion

- [Network Explorer](http://explorer.solana.com/)
- [Testnet Metrics Dashboard](https://metrics.solana.com:3000/d/monitor-edge/cluster-telemetry-edge?refresh=60s&orgId=2)
- Validator [Discord](https://solana.com/discord) channels
  - `#validator-support` --  General support channel for any Validator related queries.
  - `#testnet-announcements` -- The single source of truth for critical information relating Testnet
- [Core software repo](https://github.com/solana-labs/solana)

## Blockstore

The validator blockstore rocksdb database can be inspected using the `ldb` tool.
`ldb` is part of the `rocksdb` code base and is also available in the `rocksdb-tools`
package.

[RocksDB Administration and Data Access Tool](https://github.com/facebook/rocksdb/wiki/Administration-and-Data-Access-Tool)

## Upgrade

If a new software version introduces a new column family to the blockstore,
that new (empty) column will be automatically created. This is the same logic
that allows a validator to start fresh without the blockstore directory.

## Downgrade

If a new column family has been introduced to the validator blockstore, a
subsequent downgrade of the validator to a version that predates the new column
family will cause the validator to fail while opening the blockstore during
startup.

List column families:
```
ldb --db=<validator ledger path>/rocksdb/ list_column_families
```

**Warning**: Please seek guidance on discord before modifying the validator
blockstore.

Drop a column family:
```
ldb --db=<validator ledger path>/rocksdb drop_column_family <column family name>
```

## Common Troubleshooting

Before you approach the Discord there are a few things you can do to support the community to best support you.

Be sure to use triple back ticks ` to paste a code block of your log / config etc.

Do not use screenshots if at all possible.

Be sure to have the following ready when ready to request support:

1. Your server specifications (processor, ram, disk, networking). Make sure you've checked that your server meets  [the minimum hardware requirements here](../requirements.md). Check specifically the BASE CLOCK SPEED, the family of your processor, the number of cores, the disks and configuration of disk.
2. Your solana.sh or other CLI config you use to start your validator/RPC.
Make sure you're not using `--no-poh-test` or `--no-port-test` if you are, you're already skipping testing which is designed to do some checks even before you evaluate additional issues.
3. The location of your server.
4. What you've already attempted to do.

With this information we're more rapidly able to support your request.

Review the CLI arguments to better accquaint yourself with what each does and what you're attempting to do https://github.com/solana-labs/solana/blob/master/validator/src/cli.rs

### Validator stuck on SearchingForRPC

This can be caused due to a number of reasons. Specifically this is related to communication with your validator making a request the cluster.

If you have either `--no-untrusted-rpc` or `--only-known-rpc` this flag triggers an inclusion list which needs to be used in conjunction with `--known-validator <ACCOUNT>`. If you have these set in your config, you can try a few things to fix:
1. Use a database search (such as Validators.app) to locate validators within or near you to set as a known-validator, this can increase the probability of connection and download of the requested data.
2. Remove these configuration values to try to fetch the data from the entire cluster.
3. Use a snapshot finder or wget to download the requested data.

If you have downloaded genesis you should have `--no-genesis-fetch` added to your config.
If you've already downloaded a snapshot or externally downloaded a snapshot, you should have `--no-snapshot-fetch` added to your config.

BE AWARE - If you're unable to start your validator within 30-60 minutes it's best practice to attempt to download / fetch a new snapshot and start over.

### Validator stuck on LoadingLedger

It's not stuck, it just can take a while, review your system requirements as well as your config to ensure you're following instructions and the system meets the requirements.

### Validator unable to keep up

Review your configuration, location, networking and system specs to ensure you meet or exceed them.

#### RPC

If your running the validator in `--no-voting` as an RPC you have additional system requirements to ensure you're able to service the RPC requests. Make sure you've reviewed them.

If you have `--account-index` understand that these indexes can lead to significant performance degradation. You should be using them in conjunction with `--account-index-exclude-key <KEY>` such that you're limiting the total indexing required.

If you haven't setup a proxy or service between the RPC and internet, it may be useful for you to do so. Certain requests are known to be extremely intense and can lead to crashes and stalls in the validator. You should review what is required for your use case and understand what limits you may have to introduce to ensure smooth operation.