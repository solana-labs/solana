![image](https://user-images.githubusercontent.com/110216567/182764431-504557e4-92ac-41ff-82a5-b87c88c19c1d.png)
# Influxdb_Enterprise
[Influx_Enterprise](https://solana-labs.atlassian.net/wiki/spaces/DEVOPS/pages/25788425/Influx+Enterprise+Integration)


## Deploy an Influx Enterprise Cluster

An influx enterprise cluster requires two type of nodes, meta nodes and data notes in order to operate properly:

### Influxdb Meta Nodes

Meta nodes are the ones that keep state about the cluster, including which servers, databases, users, continuous queries, retention policies, subscriptions, and blocks of time exist.
You need at least 3 meta nodes running at all times. To replace the meta nodes or add more you can use the `setup-meta-nodes.sh` script updating the requires variables:

1. SERVERS="<LIST_OF_SERVERS>"
2. LICENSE_KEY="<YOUR_LICENSE_KEY>"
3. VERSION="<INFLUXDB_VERSION>"

### Influxdb Data Nodes

Data nodes are the ones that store all time series data and handles all writes and queries. You can have as many data nodes as possible that add up to the number on vCPU that your license allows.
To replace the data nodes or add more you can use the `setup-data-nodes.sh` script updating the requires variables:

1. SERVERS="<LIST_OF_SERVERS>"
2. LICENSE_KEY="<YOUR_LICENSE_KEY>"
3. VERSION="<INFLUXDB_VERSION>"

### Status Check

The `status.sh` script runs periodically on BuildKite to make sure that both, the data and meta services are running properly in all the servers of the cluster. If it detects that the service is not running it will try to redeploy it and send an alert to Discord and PagerDuty in case it fails to do so.
