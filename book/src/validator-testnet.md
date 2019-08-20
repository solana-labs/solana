# Choosing a Testnet
As noted in the overview, solana currently maintains several testnets, each featuring a validator that can serve as the entrypoint to the cluster for your validator.

Current testnet entrypoints:
- Stable, testnet.solana.com
- Beta, beta.testnet.solana.com
- Edge, edge.testnet.solana.com

Prior to mainnet, the testnets may be running different versions of solana
software, which may feature breaking changes. Generally, the edge testnet tracks
the tip of master, beta tracks the latest tagged minor release, and stable
tracks the most stable tagged release.

### Get Testnet Version
You can submit a JSON-RPC request to see the specific version of the cluster.
```bash
$ curl -X POST -H 'Content-Type: application/json' -d '{"jsonrpc":"2.0","id":1, "method":"getVersion"}' edge.testnet.solana.com:8899
{"jsonrpc":"2.0","result":{"solana-core":"0.18.0-pre1"},"id":1}
```

## Using a Different Testnet
This guide is written in the context of testnet.solana.com, our most stable
cluster. To participate in another testnet, you will need to modify some of the
commands in the following pages.

### Downloading Software
If you are bootstrapping with `solana-install`, you can specify the release tag or named channel to install to match your desired testnet.

```bash
$ curl -sSf https://raw.githubusercontent.com/solana-labs/solana/v0.18.0/install/solana-install-init.sh | sh -s - 0.18.0
```

```bash
$ curl -sSf https://raw.githubusercontent.com/solana-labs/solana/v0.18.0/install/solana-install-init.sh | sh -s - beta
```

Similarly, you can add this argument to the `solana-install` command if you've built the program from source:
```bash
$ solana-install init 0.18.0
```

If you are downloading pre-compiled binaries or building from source, simply choose the release matching your desired testnet.

### Validator Commands
Solana CLI tools like solana-wallet and solana-validator-info point at
testnet.solana.com by default. Include a `--url` argument to point at a
different testnet. For instance:
```bash
$ solana-wallet --url http://beta.testnet.solana.com:8899 balance
```

Solana-wallet includes `get` and `set` configuration commands to automatically
set the `--url` argument for future wallet commands.
For example:
```bash
$ solana-wallet set --url http://beta.testnet.solana.com:8899
$ solana-wallet balance # Same result as command above
```
(You can always override the set configuration by explicitly passing the `--url`
argument with a command.)

Solana-gossip and solana-validator commands already require an explicit
`--entrypoint` argument. Simply replace testnet.solana.com in the examples with
an alternate url to interact with a different testnet. For example:
```bash
$ validator.sh --identity ~/validator-keypair.json --voting-keypair ~/validator-vote-keypair.json --ledger ~/validator-config --rpc-port 8899 --poll-for-new-genesis-block beta.testnet.solana.com
```

You can also submit JSON-RPC requests to a different testnet, like:
```bash
$ curl -X POST -H 'Content-Type: application/json' -d '{"jsonrpc":"2.0","id":1, "method":"getTransactionCount"}' http://beta.testnet.solana.com:8899
```
