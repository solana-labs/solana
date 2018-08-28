
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

You currently must be running on a Linux system (for now, TODO fix this)

## Quick Start

```bash
$ cd net/

$ ./gce.sh create -n 5 -c 1  #<-- Create a GCE testnet with 5 validators, 1 client (billing starts here)
$ ./init-metrics $(whoami)   #<-- Configure a metrics database for the testnet
$ ./net.sh start             #<-- Deploy the network from the local workspace
$ ./ssh.sh                   #<-- Details on how to ssh into any testnet node
$ ./gce.sh delete            #<-- Dispose of the network (billing stops here)
```

