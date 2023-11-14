---
title: Troubleshooting
---

There is a **\#validator-support** Discord channel available to reach other
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
