# Starting a Validator

## Confirm The Testnet Is Reachable

Before attaching a validator node, sanity check that the cluster is accessible to your machine by running some simple commands. If any of the commands fail, please retry 5-10 minutes later to confirm the testnet is not just restarting itself before debugging further.

Fetch the current transaction count over JSON RPC:

```bash
curl -X POST -H 'Content-Type: application/json' -d '{"jsonrpc":"2.0","id":1, "method":"getTransactionCount"}' http://testnet.solana.com:8899
```

Inspect the network explorer at [https://explorer.solana.com/](https://explorer.solana.com/) for activity.

View the [metrics dashboard](https://metrics.solana.com:3000/d/testnet-beta/testnet-monitor-beta?var-testnet=testnet) for more detail on cluster activity.

## Confirm your Installation

Sanity check that you are able to interact with the cluster by receiving a small airdrop of lamports from the testnet drone:

```bash
solana set --url http://testnet.solana.com:8899
solana get
solana airdrop 123 lamports
solana balance --lamports
```

Also try running following command to join the gossip network and view all the other nodes in the cluster:

```bash
solana-gossip --entrypoint testnet.solana.com:8001 spy
# Press ^C to exit
```

## Enabling CUDA

If your machine has a GPU with CUDA installed \(Linux-only currently\), include the `--cuda` argument to `solana-validator`.

```bash
export SOLANA_CUDA=1
```

When your validator is started look for the following log message to indicate that CUDA is enabled: `"[<timestamp> solana::validator] CUDA is enabled"`

## Generate identity

Create an identity keypair for your validator by running:

```bash
solana-keygen new -o ~/validator-keypair.json
```

The identity public key can now be viewed by running:

```bash
    solana-keygen pubkey ~/validator-keypair.json
```

> Note: The "validator-keypair.json” file is also your \(ed25519\) private key.

Your validator identity keypair uniquely identifies your validator within the network. **It is crucial to back-up this information.**

If you don’t back up this information, you WILL NOT BE ABLE TO RECOVER YOUR VALIDATOR if you lose access to it. If this happens, YOU WILL LOSE YOUR ALLOCATION OF LAMPORTS TOO.

To back-up your validator identify keypair, **back-up your "validator-keypair.json” file to a secure location.**

### Wallet Configuration

You can set solana configuration to use your validator keypair for all following commands:

```bash
solana set --keypair ~/validator-keypair.json
```

You should see the following output:

```text
Wallet Config Updated: /home/solana/.config/solana/wallet/config.yml
* url: http://testnet.solana.com:8899
* keypair: /home/solana/validator-keypair.json
```

You can see the wallet configuration at any time by running:

```text
solana get
```

**All following solana commands assume you have set `--keypair` config** to your validator identity keypair.\*\* If you haven't, you will need to add the `--keypair` argument to each command, for example:

```bash
solana --keypair ~/validator-keypair.json airdrop 10
```

\(You can always override the set configuration by explicitly passing the `--keypair` argument with a command.\)

### Check Validator Balance

To view your current balance:

```text
solana balance
```

Or to see in finer detail:

```text
solana balance --lamports
```

Read more about the [difference between SOL and lamports here](https://solana-labs.github.io/book/introduction.html?highlight=lamport#what-are-sols).

## Validator Start

Connect to a testnet cluster by running:

```bash
export SOLANA_METRICS_CONFIG="host=https://testnet-metrics.solana.com:8086,db=testnet,u=testnet_writer,p=dry_run"
```

```bash
solana-validator --identity-keypair ~/validator-keypair.json --voting-keypair ~/validator-vote-keypair.json \
    --ledger ~/validator-ledger --rpc-port 8899 --entrypoint testnet.solana.com:8001 \
    --limit-ledger-size
```

To force validator logging to the console add a `--log -` argument, otherwise the validator will automatically log to a file.

Confirm your validator connected to the network by running:

```bash
solana-gossip spy --entrypoint testnet.solana.com:8001
```

This command will display all the nodes that are visible to the TdS network’s entrypoint. If your validator is connected, its public key and IP address will appear in the list.

Airdrop yourself some SOL to get started:

```bash
solana airdrop 1000
```

Your validator will need a vote account. Create it now with the following commands:

```bash
solana-keygen new -o ~/validator-vote-keypair.json
solana create-vote-account ~/validator-vote-keypair.json ~/validator-keypair.json
```

Then use one of the following commands, depending on your installation choice, to start the node:

If this is a `solana-install`-installation:

```bash
solana-validator --identity-keypair ~/validator-keypair.json --voting-keypair ~/validator-vote-keypair.json --ledger ~/validator-config --rpc-port 8899 --entrypoint testnet.solana.com:8001
```

Alternatively, the `solana-install run` command can be used to run the validator node while periodically checking for and applying software updates:

```bash
solana-install run solana-validator -- --identity-keypair ~/validator-keypair.json --voting-keypair ~/validator-vote-keypair.json --ledger ~/validator-config --rpc-port 8899 --entrypoint testnet.solana.com:8001
```

If you built from source:

```bash
NDEBUG=1 USE_INSTALL=1 ./multinode-demo/validator.sh --identity-keypair ~/validator-keypair.json --voting-keypair ~/validator-vote-keypair.json --rpc-port 8899 --entrypoint testnet.solana.com:8001
```

### Controlling local network port allocation

By default the validator will dynamically select available network ports in the 8000-10000 range, and may be overridden with `--dynamic-port-range`. For example, `solana-validator --dynamic-port-range 11000-11010 ...` will restrict the validator to ports 11000-11011.

### Limiting ledger size to conserve disk space

By default the validator will retain the full ledger. To conserve disk space start the validator with the `--limit-ledger-size`, which will instruct the validator to only retain the last couple hours of ledger.
