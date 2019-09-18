## Running a Replicator
This document describes how to setup a replicator in the testnet

Please note some of the information and instructions described here may change
in future releases.

### Overview
Replicators are specialized light clients. They download a part of the
ledger (a.k.a Segment) and store it. They earn rewards for storing segments.

The testnet features a validator running at testnet.solana.com, which
serves as the entrypoint to the cluster for your replicator node.

Additionally there is a blockexplorer available at
[http://testnet.solana.com/](http://testnet.solana.com/).

The testnet is configured to reset the ledger daily, or sooner
should the hourly automated cluster sanity test fail.

### Machine Requirements
Replicators don't need specialized hardware. Anything with more than
128GB of disk space will be able to participate in the cluster as a replicator node.

Currently the disk space requirements are very low but we expect them to change
in the future.

Prebuilt binaries are available for Linux x86_64 (Ubuntu 18.04 recommended),
macOS, and Windows.

#### Confirm The Testnet Is Reachable
Before starting a replicator node, sanity check that the cluster is accessible
to your machine by running some simple commands.  If any of the commands fail,
please retry 5-10 minutes later to confirm the testnet is not just restarting
itself before debugging further.

Fetch the current transaction count over JSON RPC:
```bash
$ curl -X POST -H 'Content-Type: application/json' -d '{"jsonrpc":"2.0","id":1, "method":"getTransactionCount"}' http://testnet.solana.com:8899
```

Inspect the blockexplorer at [http://testnet.solana.com/](http://testnet.solana.com/) for activity.

View the [metrics dashboard](
https://metrics.solana.com:3000/d/testnet-beta/testnet-monitor-beta?var-testnet=testnet)
for more detail on cluster activity.

### Replicator Setup
##### Obtaining The Software
##### Bootstrap with `solana-install`

The `solana-install` tool can be used to easily install and upgrade the cluster
software.

##### Linux and mac OS
```bash
$ curl -sSf https://raw.githubusercontent.com/solana-labs/solana/v0.18.0/install/solana-install-init.sh | sh -s
```

Alternatively build the `solana-install` program from source and run the
following command to obtain the same result:
```bash
$ solana-install init
```

##### Windows
Download and install **solana-install-init** from
[https://github.com/solana-labs/solana/releases/latest](https://github.com/solana-labs/solana/releases/latest)

After a successful install, `solana-install update` may be used to
easily update the software to a newer version at any time.

##### Download Prebuilt Binaries
If you would rather not use `solana-install` to manage the install, you can manually download and install the binaries.

##### Linux
Download the binaries by navigating to
[https://github.com/solana-labs/solana/releases/latest](https://github.com/solana-labs/solana/releases/latest),
download **solana-release-x86_64-unknown-linux-gnu.tar.bz2**, then extract the
archive:
```bash
$ tar jxf solana-release-x86_64-unknown-linux-gnu.tar.bz2
$ cd solana-release/
$ export PATH=$PWD/bin:$PATH
```
##### mac OS
Download the binaries by navigating to
[https://github.com/solana-labs/solana/releases/latest](https://github.com/solana-labs/solana/releases/latest),
download **solana-release-x86_64-apple-darwin.tar.bz2**, then extract the
archive:
```bash
$ tar jxf solana-release-x86_64-apple-darwin.tar.bz2
$ cd solana-release/
$ export PATH=$PWD/bin:$PATH
```
##### Windows
Download the binaries by navigating to
[https://github.com/solana-labs/solana/releases/latest](https://github.com/solana-labs/solana/releases/latest),
download **solana-release-x86_64-pc-windows-msvc.tar.bz2**, then extract it into a folder.
It is a good idea to add this extracted folder to your windows PATH.

### Starting The Replicator
Try running following command to join the gossip network and view all the other nodes in the cluster:
```bash
$ solana-gossip --entrypoint testnet.solana.com:8001 spy
# Press ^C to exit
```

Now configure the keypairs for your replicator by running:

Navigate to the solana install location and open a cmd prompt
```bash
$ solana-keygen new -o replicator-keypair.json
$ solana-keygen new -o storage-keypair.json
```

Use solana-keygen to show the public keys for each of the keypairs,
they will be needed in the next step:
- Windows
```bash
# The replicator's identity
$ solana-keygen pubkey replicator-keypair.json
$ solana-keygen pubkey storage-keypair.json
```
- Linux and mac OS
```bash
$ export REPLICATOR_IDENTITY=$(solana-keygen pubkey replicator-keypair.json)
$ export STORAGE_IDENTITY=$(solana-keygen pubkey storage-keypair.json)

```
Then set up the storage accounts for your replicator by running:
```bash
$ solana --keypair replicator-keypair.json airdrop 100000 lamports
$ solana --keypair replicator-keypair.json create-replicator-storage-account $REPLICATOR_IDENTITY $STORAGE_IDENTITY
```
Note: Every time the testnet restarts, run the steps to setup the replicator accounts again.

To start the replicator:
```bash
$ solana-replicator --entrypoint testnet.solana.com:8001 --identity replicator-keypair.json --storage-keypair storage-keypair.json --ledger replicator-ledger
```

### Verify Replicator Setup
From another console, confirm the IP address and **identity pubkey** of your replicator is visible in the
gossip network by running:
```bash
$ solana-gossip --entrypoint testnet.solana.com:8001 spy
```

Provide the **storage account pubkey** to the `solana show-storage-account` command to view
the recent mining activity from your replicator:
```bash
$ solana --keypair storage-keypair.json show-storage-account $STORAGE_IDENTITY
```
