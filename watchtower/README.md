The `solana-watchtower` program is used to monitor the health of a cluster.  It
periodically polls the cluster over an RPC API to confirm that the transaction
count is advancing, new blockhashes are available, and no validators are
delinquent.  Results are reported as InfluxDB metrics, with an optional
Slack/Discord push notification on sanity failure.

### Metrics
#### `watchtower-sanity`
On every iteration this data point will be emitted indicating the overall result
using a boolean `ok` field.

#### `watchtower-sanity-failure`
On failure this data point contains details about the specific test that failed via
the following fields:
* `test`: name of the sanity test that failed
* `err`: exact sanity failure message


### Sanity failure push notification
To receive a Slack and/or Discord notification on sanity failure, define one or
both of these environment variables before running `solana-watchtower`:
```
export SLACK_WEBHOOK=...
export DISCORD_WEBHOOK=...
```
