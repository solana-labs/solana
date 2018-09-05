
# Network Management
This directory contains scripts useful for working with a test network.  It's
intended to be both dev and CD friendly.

### User Account Prerequisites

Log in to GCP with:
```bash
$ gcloud auth login
```

Also ensure that `$(whoami)` is the name of an InfluxDB user account with enough
access to create a new database.

## Quick Start
```bash
$ cd net/
$ ./gce.sh create -n 5 -c 1  #<-- Create a GCE testnet with 5 validators, 1 client (billing starts here)
$ ./init-metrics $(whoami)   #<-- Configure a metrics database for the testnet
$ ./net.sh start             #<-- Deploy the network from the local workspace
$ ./ssh.sh                   #<-- Details on how to ssh into any testnet node
$ ./gce.sh delete            #<-- Dispose of the network (billing stops here)
```

## Tips

### Running the network over public IP addresses
By default private IP addresses are used with all instances in the same
availability zone to avoid GCE network engress charges.  However to run the
network over public IP addresses:
```bash
$ ./gce create -P ...
```

### Deploying a Snap-based network
To deploy the latest pre-build `edge` channel Snap (ie, latest from the `master`
branch), once the testnet has been created run:

```bash
$ ./net start -s edge
```

### Enabling CUDA
First ensure the network instances are created with GPU enabled:
```bash
$ ./gce.sh create -g ...
```

If deploying a Snap-based network nothing further is required, as GPU presence
is detected at runtime and the CUDA build is auto selected.

If deploying a locally-built network, first run `./fetch-perf-libs.sh` then
ensure the `cuda` feature is specified at network start:
```bash
$ ./net.sh start -f "cuda,erasure"
```

