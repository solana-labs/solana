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

## Using a Different Testnet
This guide is written to point at testnet.solana.com. To participate in another
testnet, you will need to modify some of the commands.

Solana CLI tools like solana-wallet and solana-validator-info point at
testnet.solana.com by default. Include a `--url` argument to point at a
different testnet. For instance:
```bash
$ solana-wallet --url http://beta.testnet.solana.com:8899 balance
```

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
