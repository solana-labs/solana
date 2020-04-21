# Connecting to a Cluster
See [Solana Clusters](../clusters.md) for general information about the
available clusters.

## Configure the command-line tool
You can check what cluster the Solana command-line tool (CLI) is currently targeting by
running the following command:

```bash
solana config get
```

Use `solana config set` command to target a particular cluster.  After setting
a cluster target, any future subcommands will send/receive information from that
cluster.

For example to target the Devnet cluster, run:
```bash
solana config set --url https://devnet.solana.com
```

## Ensure Versions Match

Though not strictly necessary, the CLI will generally work best when its version
matches the software version running on the cluster. To get the locally-installed
CLI version, run:

```bash
solana --version
```

To get the cluster version, run:

```bash
solana cluster-version
```

Ensure the local CLI version is greater than or equal to the cluster version.
