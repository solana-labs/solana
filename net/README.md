
# Network Management
This directory contains scripts useful for working with a test network.  It's
intended to be both dev and CD friendly.

### User Account Prerequisites

GCP and AWS are supported.

#### GCP
First authenticate with
```bash
$ gcloud auth login
```

#### AWS
Obtain your credentials from the AWS IAM Console and configure the AWS CLI with
```bash
$ aws configure
```
More information on AWS CLI configuration can be found [here](https://docs.aws.amazon.com/cli/latest/userguide/cli-chap-getting-started.html#cli-quick-configuration)

### Metrics configuration (Optional)
Ensure that `$(whoami)` is the name of an InfluxDB user account with enough
access to create a new InfluxDB database.  Ask mvines@ for help if needed.

## Quick Start

NOTE: This example uses GCE.  If you are using AWS EC2, replace `./gce.sh` with
`./ec2.sh` in the commands.

```bash
$ cd net/
$ ./gce.sh create -n 5 -c 1     #<-- Create a GCE testnet with 5 additional nodes (beyond the bootstrap node) and 1 client (billing starts here)
$ ./init-metrics.sh $(whoami)   #<-- Configure a metrics database for the testnet
$ ./net.sh start                #<-- Deploy the network from the local workspace and start all clients with bench-tps
$ ./ssh.sh                      #<-- Details on how to ssh into any testnet node to access logs/etc
$ ./gce.sh delete               #<-- Dispose of the network (billing stops here)
```

## Tips

### Running the network over public IP addresses
By default private IP addresses are used with all instances in the same
availability zone to avoid GCE network engress charges.  However to run the
network over public IP addresses:
```bash
$ ./gce.sh create -P ...
```
or
```bash
$ ./ec2.sh create -P ...
```

### Deploying a tarball-based network
To deploy the latest pre-built `edge` channel tarball (ie, latest from the `master`
branch), once the testnet has been created run:

```bash
$ ./net.sh start -t edge
```

### Enabling CUDA
First ensure the network instances are created with GPU enabled:
```bash
$ ./gce.sh create -g ...
```
or
```bash
$ ./ec2.sh create -g ...
```

If deploying a tarball-based network nothing further is required, as GPU presence
is detected at runtime and the CUDA build is auto selected.

If deploying a locally-built network, first run `./fetch-perf-libs.sh` then
ensure the `cuda` feature is specified at network start:
```bash
$ ./net.sh start -f "cuda"
```

### How to interact with a CD testnet deployed by ci/testnet-deploy.sh

**AWS-Specific Extra Setup**: Follow the steps in
`scripts/solana-user-authorized_keys.sh`, then redeploy the testnet
before continuing in this section.

Taking **master-testnet-solana-com** as an example, configure your workspace for
the testnet using:
```bash
$ ./gce.sh config -p master-testnet-solana-com
```
or
```bash
$ ./ec2.sh config -p master-testnet-solana-com
```

Then run the following for details on how to ssh into any testnet node to access logs or otherwise inspect the node
```bash
$ ./ssh.sh
```
