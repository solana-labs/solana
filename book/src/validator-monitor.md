# Validator Monitoring
When `validator.sh` starts, it will output a validator configuration that looks
similar to:
```bash
======================[ validator configuration ]======================
identity pubkey: 4ceWXsL3UJvn7NYZiRkw7NsryMpviaKBDYr8GK7J61Dm
vote pubkey: 2ozWvfaXQd1X6uKh8jERoRGApDqSqcEy6fF1oN13LL2G
ledger: ...
accounts: ...
======================================================================
```

The **identity pubkey** for your validator can also be found by running:
```bash
$ solana-keygen pubkey ~/validator-keypair.json
```

From another console, confirm the IP address and **identity pubkey** of your
validator is visible in the gossip network by running:
```bash
$ solana-gossip --entrypoint testnet.solana.com:8001 spy
```

Provide the **vote pubkey** to the `solana-wallet show-vote-account` command to view
the recent voting activity from your validator:
```bash
$ solana-wallet show-vote-account 2ozWvfaXQd1X6uKh8jERoRGApDqSqcEy6fF1oN13LL2G
```

The vote pubkey for the validator can also be found by running:
```bash
# If this is a `solana-install`-installation run:
$ solana-keygen pubkey ~/.local/share/solana/install/active_release/config-local/validator-vote-keypair.json
# Otherwise run:
$ solana-keygen pubkey ./config-local/validator-vote-keypair.json
```

Your lamport balance should decrease by the transaction fee amount as your
validator submits votes, and increase after serving as the leader:
```bash
$ solana-wallet --keypair ~/validator-keypair.json
```

## Validator Metrics
Metrics are available for local monitoring of your validator.

Docker must be installed and the current user added to the docker group.  Then
download `solana-metrics.tar.bz2` from the Github Release and run
```bash
$ tar jxf solana-metrics.tar.bz2
$ cd solana-metrics/
$ ./start.sh
```

A local InfluxDB and Grafana instance is now running on your machine.  Define
`SOLANA_METRICS_CONFIG` in your environment as described at the end of the
`start.sh` output and restart your validator.

Metrics should now be streaming and visible from your local Grafana dashboard.

## Timezone For Log Messages
Log messages emitted by your validator include a timestamp.  When sharing logs
with others to help triage issues, that timestamp can cause confusion as it does
not contain timezone information.

To make it easier to compare logs between different sources we request that
everybody use Pacific Time on their validator nodes.  In Linux this can be
accomplished by running:
```bash
$ sudo ln -sf /usr/share/zoneinfo/America/Los_Angeles /etc/localtime
```
