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

## Check Gossip
The **identity pubkey** for your validator can also be found by running:
```bash
$ solana-keygen pubkey ~/validator-keypair.json
```

From another console, confirm the IP address and **identity pubkey** of your
validator is visible in the gossip network by running:
```bash
$ solana-gossip --entrypoint testnet.solana.com:8001 spy
```

## Check Vote Activity
The vote pubkey for the validator can also be found by running:
```bash
$ solana-keygen pubkey ~/validator-vote-keypair.json
```

Provide the **vote pubkey** to the `solana-wallet show-vote-account` command to view
the recent voting activity from your validator:
```bash
$ solana-wallet show-vote-account 2ozWvfaXQd1X6uKh8jERoRGApDqSqcEy6fF1oN13LL2G
```

## Check Your Balance
Your lamport balance should decrease by the transaction fee amount as your
validator submits votes, and increase after serving as the leader:
```bash
$ solana-wallet --keypair ~/validator-keypair.json
```

## Check Slot Number
After your validator boots, it may take some time to catch up with the cluster.
Use the `get-slot` wallet command to view the current slot that the cluster is
processing:
```bash
$ solana-wallet get-slot
```

The current slot that your validator is processing can then been seen with:
```bash
$ solana-wallet --url http://127.0.0.1:8899 get-slot
```

Until your validator has caught up, it will not be able to vote successfully and
stake cannot be delegated to it.

Also if you find the cluster's slot advancing faster than yours, you will likely
never catch up.  This typically implies some kind of networking issue between
your validator and the rest of the cluster.

## Get Cluster Info
There are several useful JSON-RPC endpoints for monitoring your validator on the
cluster, as well as the health of the cluster:

```bash
# Similar to solana-gossip, you should see your validator in the list of cluster nodes
$ curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc":"2.0","id":1, "method":"getClusterNodes"}' http://testnet.solana.com:8899
# If your validator is properly staked and voting, it should appear in the list of epoch vote accounts
$ curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc":"2.0","id":1, "method":"getEpochVoteAccounts"}' http://testnet.solana.com:8899
# Returns the current leader schedule
$ curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc":"2.0","id":1, "method":"getLeaderSchedule"}' http://testnet.solana.com:8899
# Returns info about the current epoch. slotIndex should progress on subsequent calls.
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc":"2.0","id":1, "method":"getEpochInfo"}' http://testnet.solana.com:8899
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
