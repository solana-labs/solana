![image](https://user-images.githubusercontent.com/110216567/184346286-94e0b45f-19e9-4fc9-a1a3-2e50c6f12bf8.png)

# Metrics

## InfluxDB

In oder to explore validator specific metrics from mainnet-beta, testnet or devnet you can use Chronograf:

* https://metrics.solana.com:8888/ (production enviroment)
* https://metrics.solana.com:8889/ (testing enviroment)

For local cluster deployments you should use:

* https://internal-metrics.solana.com:8888/
* https://internal-metrics.solana.com:8889/

## Public Grafana Dashboards

There are three main public dashboards for cluster related metrics:

* https://metrics.solana.com/d/monitor-edge/cluster-telemetry
* https://metrics.solana.com/d/0n54roOVz/fee-market
* https://metrics.solana.com/d/UpIWbId4k/ping-result

For local cluster deployments you should use:

* https://internal-metrics.solana.com:3000/

### Cluster Telemetry

The cluster telemetry dashboard shows the current state of the cluster:

1. Cluster Stability
2. Validator Streamer
3. Tomer Consensus
4. IP Network
5. Snapshots
6. RPC Send Transaction Service

### Fee Market

The fee market dashboard shows:

1. Total Priorization Fees
2. Block Min Priorization Fees
3. Cost Tracker Stats

### Ping Results

The ping reults dashboard displays relevant information about the Ping API
