# Choosing a Testnet

As noted in the overview, solana currently maintains several testnets, each
featuring a validator that can serve as the entrypoint to the cluster for your
validator.

Current testnet entrypoints:

* Stable, testnet.solana.com
* Beta, beta.testnet.solana.com
* Edge, edge.testnet.solana.com

Prior to mainnet, the testnets may be running different versions of solana
software, which may feature breaking changes. Generally, the edge testnet tracks
the tip of master, beta tracks the latest tagged minor release, and stable
tracks the most stable tagged release.

### Get Testnet Version

You can submit a JSON-RPC request to see the specific software version of the
cluster. Use this to specify [the software version to install](validator-software.md).

```bash
curl -X POST -H 'Content-Type: application/json' -d '{"jsonrpc":"2.0","id":1, "method":"getVersion"}' testnet.solana.com:8899
```
Example result:
`{"jsonrpc":"2.0","result":{"solana-core":"0.21.0"},"id":1}`

## Using a Different Testnet

This guide is written in the context of testnet.solana.com, our most stable
cluster. To participate in another testnet, modify the commands in the following
pages, replacing `testnet.solana.com` with your desired testnet.
