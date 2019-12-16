
This directory contains scripts to manage a local instance of [InfluxDB OSS](https://docs.influxdata.com/influxdb/v1.6/) and [Grafana](https://grafana.com/docs/v5.2/)

### Setup

Start the local metric services:

`$ ./start.sh`

Metrics are enabled on a per-shell basis which means you must `source` the
following scripts in each shell in which you start an application you wish to
collect metrics from.  For example, if running a Solana validator you must run
`source ./enable.sh` before starting the node:

`$ source ./enable.sh`

Once metrics have been started and you have an application running you can view the metrics at:

http://localhost:3000/dashboards

To test that things are working correctly you can send a test airdrop data point and then check the
metrics dashboard:

`$ ./test.sh`

Stop metric services:

`$ ./stop.sh`

### InfluxDB CLI

You may find it useful to install the InfluxDB client for
adhoc metrics collection/viewing
* Linux - `sudo apt-get install influxdb-client`
* macOS - `brew install influxdb`

Simple example of pulling all airdrop measurements out of the `testnet` database:

```sh
$ influx -database testnet -username read -password read -execute 'SELECT * FROM "faucet-airdrop"'
```

Reference: https://docs.influxdata.com/influxdb/v1.5/query_language/

### Monitoring

To monitor activity, run one of:

```sh
$ docker logs -f influxdb
$ docker logs -f grafana
```

### Reference
* https://hub.docker.com/_/influxdata-influxdb
* https://hub.docker.com/r/grafana/grafana
