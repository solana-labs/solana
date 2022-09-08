## Canaries
In order to reduce the risk associated with deploying updates Solana Labs operates the validator software on canary nodes on mainnet-beta
and testnet. The mainnet-beta nodes are non-voting. These nodes update themselves on a regular schedule.

|Canary|Host|Dashboards|Identity|Cluster|Channel|Days between updates|
|------|----|----------|--------|-------|-------|-------------------:|
|mce1|edge-validator-us-sv15|[Cluster](https://metrics.solana.com:3000/d/monitor-beta/cluster-telemetry-beta?orgId=1&var-datasource=InfluxDB_main-beta&var-testnet=mainnet-beta&var-hostid=edge8WksfN71qYDJ1e4fCy2WfKg19fXU5zuztDi9uTM) [System](https://metrics.solana.com:3000/d/rYdddlPWkk/system-metrics-full?orgId=1&var-DS_PROMETHEUS=Metrics-Prometheus&var-job=All&var-node=edge-validator-us-sv15:9100&var-diskdevices=%5Ba-z%5D%2B%7Cnvme%5B0-9%5D%2Bn%5B0-9%5D%2B&refresh=5m&from=now-15m&to=now)|edge8WksfN71qYDJ1e4fCy2WfKg19fXU5zuztDi9uTM|mainnet-beta|edge|2|
|mce2|canary-am6-2|[Cluster](https://metrics.solana.com:3000/d/monitor-beta/cluster-telemetry-beta?orgId=1&var-datasource=InfluxDB_main-beta&var-testnet=mainnet-beta&var-hostid=mce2CKApCodefxDBUDWXCdBkqoh2dg1vpWJJX2qfuvV) [System](https://metrics.solana.com:3000/d/rYdddlPWkk/system-metrics-full?orgId=1&var-DS_PROMETHEUS=Metrics-Prometheus&var-job=All&var-node=canary-am6-2:9100&var-diskdevices=%5Ba-z%5D%2B%7Cnvme%5B0-9%5D%2Bn%5B0-9%5D%2B&from=now-15m&to=now&refresh=5m)|mce2CKApCodefxDBUDWXCdBkqoh2dg1vpWJJX2qfuvV|mainnet-beta|edge|4|
|mce3|canary-da11-1|[Cluster](https://metrics.solana.com:3000/d/monitor-beta/cluster-telemetry-beta?orgId=1&var-datasource=InfluxDB_main-beta&var-testnet=mainnet-beta&var-hostid=mce3QKzeRNwk3TC6XfHTy5hdRT6u5UKm4rKQbNKkFhF) [System](https://metrics.solana.com:3000/d/rYdddlPWkk/system-metrics-full?orgId=1&var-DS_PROMETHEUS=Metrics-Prometheus&var-job=All&var-node=canary-da11-1%3A9100&var-diskdevices=%5Ba-z%5D%2B%7Cnvme%5B0-9%5D%2Bn%5B0-9%5D%2B&refresh=5m&from=now-15m&to=now)|mce3QKzeRNwk3TC6XfHTy5hdRT6u5UKm4rKQbNKkFhF|mainnet-beta|edge|8|
|mcb1|beta-validator-us-ny5|[Cluster](https://metrics.solana.com:3000/d/monitor-beta/cluster-telemetry-beta?orgId=1&var-datasource=InfluxDB_main-beta&var-testnet=mainnet-beta&var-hostid=betaVcnkBhHKaWx9o6LoSYrGaoDCskQLm94cUVWqDLS) [System](https://metrics.solana.com:3000/d/rYdddlPWkk/system-metrics-full?orgId=1&var-DS_PROMETHEUS=Metrics-Prometheus&var-job=Mainnet-Beta&var-node=beta-validator-us-ny5%3A9100&var-diskdevices=%5Ba-z%5D%2B%7Cnvme%5B0-9%5D%2Bn%5B0-9%5D%2B&refresh=5m&from=now-15m&to=now)|betaVcnkBhHKaWx9o6LoSYrGaoDCskQLm94cUVWqDLS|mainnet-beta|beta|2|
|mcb2|canary-ny5-2|[Cluster](https://metrics.solana.com:3000/d/monitor-beta/cluster-telemetry-beta?orgId=1&var-datasource=InfluxDB_main-beta&var-testnet=mainnet-beta&var-hostid=mcb2qZRoYgy4bJkPRv6cBwLAAYow9ZsSzcrjJKprUnd) [System](https://metrics.solana.com:3000/d/rYdddlPWkk/system-metrics-full?orgId=1&var-DS_PROMETHEUS=Metrics-Prometheus&var-job=All&var-node=canary-ny5-2:9100&var-diskdevices=%5Ba-z%5D%2B%7Cnvme%5B0-9%5D%2Bn%5B0-9%5D%2B&refresh=5m&from=now-15m&to=now)|mcb2qZRoYgy4bJkPRv6cBwLAAYow9ZsSzcrjJKprUnd|mainnet-beta|beta|4|
|mcs1|canary-am6-1|[Cluster](https://metrics.solana.com:3000/d/monitor-beta/cluster-telemetry-beta?orgId=1&var-datasource=InfluxDB_main-beta&var-testnet=mainnet-beta&var-hostid=mcs1kpUkWeqoruxWwtCskY1GGF4Bx1t3MMtHSHoSLyC) [System](https://metrics.solana.com:3000/d/rYdddlPWkk/system-metrics-full?orgId=1&var-DS_PROMETHEUS=Metrics-Prometheus&var-job=All&var-node=canary-am6-1:9100&var-diskdevices=%5Ba-z%5D%2B%7Cnvme%5B0-9%5D%2Bn%5B0-9%5D%2B&refresh=5m&from=now-15m&to=now)|mcs1kpUkWeqoruxWwtCskY1GGF4Bx1t3MMtHSHoSLyC|mainnet-beta|stable|2|
|mcs2|canary-ny5-1|[Cluster](https://metrics.solana.com:3000/d/monitor-beta/cluster-telemetry-beta?orgId=1&var-datasource=InfluxDB_main-beta&var-testnet=mainnet-beta&var-hostid=mcs2ZZa1vHhbUyfZ6ttHEGpFU6pib4pm4ownTxBm6Jc) [System](https://metrics.solana.com:3000/d/rYdddlPWkk/system-metrics-full?orgId=1&var-DS_PROMETHEUS=Metrics-Prometheus&var-job=All&var-node=canary-ny5-1:9100&var-diskdevices=%5Ba-z%5D%2B%7Cnvme%5B0-9%5D%2Bn%5B0-9%5D%2B&refresh=5m&from=now-15m&to=now)|mcs2ZZa1vHhbUyfZ6ttHEGpFU6pib4pm4ownTxBm6Jc|mainnet-beta|stable|4|
|tce1|canary-sg1-1|[Cluster](https://metrics.solana.com:3000/d/monitor-beta/cluster-telemetry-beta?orgId=1&var-datasource=InfluxDB-testnet&var-testnet=tds&var-hostid=tce1TNB7pchMpM36eyzvttBDEwTczv86o5P2SS8dpSU) [System](https://metrics.solana.com:3000/d/rYdddlPWkk/system-metrics-full?orgId=1&var-DS_PROMETHEUS=Metrics-Prometheus&var-job=All&var-node=canary-sg1-1:9100&var-diskdevices=%5Ba-z%5D%2B%7Cnvme%5B0-9%5D%2Bn%5B0-9%5D%2B&refresh=5m&from=now-15m&to=now)|tce1TNB7pchMpM36eyzvttBDEwTczv86o5P2SS8dpSU|testnet|edge|4|
|tcb1|canary-sv15-1|[Cluster](https://metrics.solana.com:3000/d/monitor-beta/cluster-telemetry-beta?orgId=1&var-datasource=InfluxDB-testnet&var-testnet=tds&var-hostid=tcb1JYngtdwigQaJV6t13TJSnKuEPitpwoHS5TAYg1H) [System](https://metrics.solana.com:3000/d/rYdddlPWkk/system-metrics-full?orgId=1&var-DS_PROMETHEUS=Metrics-Prometheus&var-job=All&var-node=canary-sv15-1:9100&var-diskdevices=%5Ba-z%5D%2B%7Cnvme%5B0-9%5D%2Bn%5B0-9%5D%2B&refresh=5m&from=now-15m&to=now)|tcb1JYngtdwigQaJV6t13TJSnKuEPitpwoHS5TAYg1H|testnet|beta|4|

The canaries update themselves every few days according to the schedule above. They should not be updated manually. The updates get the newest build for the given channel which is often not a tagged release.

These are handy commands to see what versions are currently running on each  node:
```
solana gossip -um | grep -E " (edge|beta|mc[ebs]\d)"

solana gossip -ut | grep -E " (tc[eb]\d)"
```

Nodes update themselves when `(number of days since 1970-01-01)` mod `(days between updates)` is 0. Use this command (and the table above) to check when a node will update:

```
DAYS_BETWEEN_UPDATES=8; d=$(expr $(date +%s) / 86400 % $DAYS_BETWEEN_UPDATES); n=$(expr $DAYS_BETWEEN_UPDATES - $d); echo "Updated $d day(s) ago. Will update $n day(s) from now"
```

# Alerts
Canaries are monitored by watchtower and alert in Slack #pager-duty-canary
