## Canaries
In order to reduce the risk associated with deploying updates Solana Labs operates the validator software on canary nodes on mainnet-beta
and testnet. These nodes update themselves on a regular schedule.

|Canary|Host|Dashboards|Identity|Cluster|Channel|Updates on|
|------|----|----------|--------|-------|-------|-------------------:|
|mce1|edge-validator-us-sv15|[Cluster](https://metrics.solana.com:3000/d/monitor-beta/cluster-telemetry-beta?orgId=1&var-datasource=InfluxDB_main-beta&var-testnet=mainnet-beta&var-hostid=edge8WksfN71qYDJ1e4fCy2WfKg19fXU5zuztDi9uTM) [System](https://metrics.solana.com:3000/d/rYdddlPWkk/system-metrics-full?orgId=1&var-DS_PROMETHEUS=Metrics-Prometheus&var-job=All&var-node=edge-validator-us-sv15:9100&var-diskdevices=%5Ba-z%5D%2B%7Cnvme%5B0-9%5D%2Bn%5B0-9%5D%2B&refresh=5m&from=now-15m&to=now)|edge8WksfN71qYDJ1e4fCy2WfKg19fXU5zuztDi9uTM|mainnet-beta|edge|Monday|
|mce2|canary-am6-2|[Cluster](https://metrics.solana.com:3000/d/monitor-beta/cluster-telemetry-beta?orgId=1&var-datasource=InfluxDB_main-beta&var-testnet=mainnet-beta&var-hostid=mce2CKApCodefxDBUDWXCdBkqoh2dg1vpWJJX2qfuvV) [System](https://metrics.solana.com:3000/d/rYdddlPWkk/system-metrics-full?orgId=1&var-DS_PROMETHEUS=Metrics-Prometheus&var-job=All&var-node=canary-am6-2:9100&var-diskdevices=%5Ba-z%5D%2B%7Cnvme%5B0-9%5D%2Bn%5B0-9%5D%2B&from=now-15m&to=now&refresh=5m)|mce2CKApCodefxDBUDWXCdBkqoh2dg1vpWJJX2qfuvV|mainnet-beta|edge|Tuesday|
|mce3|canary-da11-1|[Cluster](https://metrics.solana.com:3000/d/monitor-beta/cluster-telemetry-beta?orgId=1&var-datasource=InfluxDB_main-beta&var-testnet=mainnet-beta&var-hostid=mce3QKzeRNwk3TC6XfHTy5hdRT6u5UKm4rKQbNKkFhF) [System](https://metrics.solana.com:3000/d/rYdddlPWkk/system-metrics-full?orgId=1&var-DS_PROMETHEUS=Metrics-Prometheus&var-job=All&var-node=canary-da11-1%3A9100&var-diskdevices=%5Ba-z%5D%2B%7Cnvme%5B0-9%5D%2Bn%5B0-9%5D%2B&refresh=5m&from=now-15m&to=now)|mce3QKzeRNwk3TC6XfHTy5hdRT6u5UKm4rKQbNKkFhF|mainnet-beta|edge|Wednesday|
|mce4|canary-helsinki-1||mce4bwA16BhCBdpCoByb5BMiwB5hTTrhB2acLxr54PZ|mainnet-beta|edge|Thursday|
|mce5|canary-helsinki-2||mce51pFFGjrTnB5gZ2JAQvjojniRbV7MaHr9ywvVZcD|mainnet-beta|edge|Friday|
|mce6|canary-sv16-1||mce6ixPqkQTXMhaQsUCtXV23JC3JLCeyqbQGoNmgNCX|mainnet-beta|edge|Saturday|
|mce7|canary-am6-3||mce7sh9jC6mxekMePsZNFqHaWn7o8Jah14KPpdEaU2e|mainnet-beta|edge|Sunday|
|mcb1|beta-validator-us-ny5|[Cluster](https://metrics.solana.com:3000/d/monitor-beta/cluster-telemetry-beta?orgId=1&var-datasource=InfluxDB_main-beta&var-testnet=mainnet-beta&var-hostid=betaVcnkBhHKaWx9o6LoSYrGaoDCskQLm94cUVWqDLS) [System](https://metrics.solana.com:3000/d/rYdddlPWkk/system-metrics-full?orgId=1&var-DS_PROMETHEUS=Metrics-Prometheus&var-job=Mainnet-Beta&var-node=beta-validator-us-ny5%3A9100&var-diskdevices=%5Ba-z%5D%2B%7Cnvme%5B0-9%5D%2Bn%5B0-9%5D%2B&refresh=5m&from=now-15m&to=now)|betaVcnkBhHKaWx9o6LoSYrGaoDCskQLm94cUVWqDLS|mainnet-beta|beta|Monday|
|mcb2|canary-ny5-2|[Cluster](https://metrics.solana.com:3000/d/monitor-beta/cluster-telemetry-beta?orgId=1&var-datasource=InfluxDB_main-beta&var-testnet=mainnet-beta&var-hostid=mcb2qZRoYgy4bJkPRv6cBwLAAYow9ZsSzcrjJKprUnd) [System](https://metrics.solana.com:3000/d/rYdddlPWkk/system-metrics-full?orgId=1&var-DS_PROMETHEUS=Metrics-Prometheus&var-job=All&var-node=canary-ny5-2:9100&var-diskdevices=%5Ba-z%5D%2B%7Cnvme%5B0-9%5D%2Bn%5B0-9%5D%2B&refresh=5m&from=now-15m&to=now)|mcb2qZRoYgy4bJkPRv6cBwLAAYow9ZsSzcrjJKprUnd|mainnet-beta|beta|Tuesday|
|mcb3|canary-fr2-1||mcb3Vr2Gv3TFLxwxbFXpS13kC349JNTmxErqu7V23Bv|mainnet-beta|beta|Wednesday|
|mcb4|canary-am6-4||mcb4JihYNgbhJfc8Un6JEjSEqBrGkmKGev38G55U5qL|mainnet-beta|beta|Thursday|
|mcb5|canary-chicago-3||mcb5i4hCFKHK26z1pCbAYBB2efHnS2EZMv2bRfFaL7x|mainnet-beta|beta|Friday|
|mcb6|canary-chicago-4||mcb6FGCrpQUVEBzVSd7e2goVAyy7TjsRsu9E2rXFN7h|mainnet-beta|beta|Saturday|
|mcb7|canary-fr2-2||mcb7TbA89yxy79EW7NKddzGL7YZDeccykbu1LJLBT73|mainnet-beta|beta|Sunday|
|mcs1|canary-am6-1|[Cluster](https://metrics.solana.com:3000/d/monitor-beta/cluster-telemetry-beta?orgId=1&var-datasource=InfluxDB_main-beta&var-testnet=mainnet-beta&var-hostid=mcs1kpUkWeqoruxWwtCskY1GGF4Bx1t3MMtHSHoSLyC) [System](https://metrics.solana.com:3000/d/rYdddlPWkk/system-metrics-full?orgId=1&var-DS_PROMETHEUS=Metrics-Prometheus&var-job=All&var-node=canary-am6-1:9100&var-diskdevices=%5Ba-z%5D%2B%7Cnvme%5B0-9%5D%2Bn%5B0-9%5D%2B&refresh=5m&from=now-15m&to=now)|mcs1kpUkWeqoruxWwtCskY1GGF4Bx1t3MMtHSHoSLyC|mainnet-beta|stable|Monday|
|mcs2|canary-ny5-1|[Cluster](https://metrics.solana.com:3000/d/monitor-beta/cluster-telemetry-beta?orgId=1&var-datasource=InfluxDB_main-beta&var-testnet=mainnet-beta&var-hostid=mcs2ZZa1vHhbUyfZ6ttHEGpFU6pib4pm4ownTxBm6Jc) [System](https://metrics.solana.com:3000/d/rYdddlPWkk/system-metrics-full?orgId=1&var-DS_PROMETHEUS=Metrics-Prometheus&var-job=All&var-node=canary-ny5-1:9100&var-diskdevices=%5Ba-z%5D%2B%7Cnvme%5B0-9%5D%2Bn%5B0-9%5D%2B&refresh=5m&from=now-15m&to=now)|mcs2ZZa1vHhbUyfZ6ttHEGpFU6pib4pm4ownTxBm6Jc|mainnet-beta|stable|Tuesday|
|mcs3|canary-hk2-1||mcs3sNKXMfXcRJu1e7mg8DUW2nn65XSkoKc6DAkcPjW|mainnet-beta|stable|Wednesday|
|mcs4|canary-hk2-2||mcs41tmU4qwhjcPm7hkdCWNyXUsizxF6LcNjSLmPdvm|mainnet-beta|stable|Thursday|
|mcs5|canary-ny5-3||mcs5vHjxGNoh7j7AUCVtgT6fe2ChTpCHJggRSot1hFm|mainnet-beta|stable|Friday|
|mcs6|canary-sg1-2||mcs6wEJ9XzDYsubraPatc34ASa3jN5vfNhhrXzfsGkQ|mainnet-beta|stable|Saturday|
|mcs7|canary-tokyo-2||mcs7JXa7TykTPoeGFqPmwqKQ1yaWopxTeUU1JdfssG7|mainnet-beta|stable|Sunday|
|tce1|canary-sg1-1|[Cluster](https://metrics.solana.com:3000/d/monitor-beta/cluster-telemetry-beta?orgId=1&var-datasource=InfluxDB-testnet&var-testnet=tds&var-hostid=tce1TNB7pchMpM36eyzvttBDEwTczv86o5P2SS8dpSU) [System](https://metrics.solana.com:3000/d/rYdddlPWkk/system-metrics-full?orgId=1&var-DS_PROMETHEUS=Metrics-Prometheus&var-job=All&var-node=canary-sg1-1:9100&var-diskdevices=%5Ba-z%5D%2B%7Cnvme%5B0-9%5D%2Bn%5B0-9%5D%2B&refresh=5m&from=now-15m&to=now)|tce1TNB7pchMpM36eyzvttBDEwTczv86o5P2SS8dpSU|testnet|edge|4|
|tcb1|canary-sv15-1|[Cluster](https://metrics.solana.com:3000/d/monitor-beta/cluster-telemetry-beta?orgId=1&var-datasource=InfluxDB-testnet&var-testnet=tds&var-hostid=tcb1JYngtdwigQaJV6t13TJSnKuEPitpwoHS5TAYg1H) [System](https://metrics.solana.com:3000/d/rYdddlPWkk/system-metrics-full?orgId=1&var-DS_PROMETHEUS=Metrics-Prometheus&var-job=All&var-node=canary-sv15-1:9100&var-diskdevices=%5Ba-z%5D%2B%7Cnvme%5B0-9%5D%2Bn%5B0-9%5D%2B&refresh=5m&from=now-15m&to=now)|tcb1JYngtdwigQaJV6t13TJSnKuEPitpwoHS5TAYg1H|testnet|beta|4|
|tcs1|||tcs1Sv111hCYw4XVXejjmWybne4arptjgbgnGAKwWwc|testnet|stable||

|Staked Canary|Host|Dashboards|Identity|Cluster|Channel|Updates on|
|------|----|----------|--------|-------|-------|-------------------:|
|sce1|canary-chicago-1||sce1ox9ZqNtCnki8fdcGaCtRGZfQQ8WFcUf33G2hhhQ|mainnet-beta|edge|Monday|
|sce2|canary-chicago-2||sce2UV31mbj5wCMg8W1dywCVNVdswjrL9iYUAhwbXqp|mainnet-beta|edge|Wednesday|
|sce3|canary-newyork-1||sce3toTHRs1ctHw5QTtfE8N8Z8WL6cTnfzJyt2c1bJX|mainnet-beta|edge|Friday|
|scb1|canary-newyork-2||scb1ZM4YVbg8PjGfoAQ1fbC3EvPt19nZBTYF6kakZH9|mainnet-beta|beta|Monday|
|scb2|canary-dallas-1||scb2CVc7oXA8MerNhyTje8sh3fWR7hzXuzA9yqW62Tr|mainnet-beta|beta|Thursday|
|scs1|canary-dallas-2||scs1AH1aqgAPRUZTAGUGq7vH74i2Mkzfh83Yk4Fo6cp|mainnet-beta|stable|Monday|
|scs2|canary-washington-1||scs2LLx4ySqYexe26cSgQXcZncrp8esrFHH3R3bXrVc|mainnet-beta|stable|Thursday|

The canaries update themselves every few days according to the schedule above. They should not be updated manually.

These are handy commands to see what versions are currently running on each  node:
```
solana gossip -um | grep -E " (edge|beta|[sm]c[ebs]\d)"

solana gossip -ut | grep -E " (tc[ebs]\d)"
```

# Alerts
