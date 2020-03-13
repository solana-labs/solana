# Connect to the Solana network

## Create Vote Account

Once you’ve confirmed the network is running, it’s time to connect your validator to the network.

If you haven’t already done so, create a vote-account keypair and create the vote account on the network. If you have completed this step, you should see the “vote-account-keypair.json” in your Solana runtime directory:

```bash
solana-keygen new -o ~/vote-account-keypair.json
```

Create your vote account on the blockchain:

```bash
solana create-vote-account ~/vote-account-keypair.json ~/validator-keypair.json
```

## Connect Your Validator

Connect to the Tour de SOL cluster by running:

```bash
export SOLANA_METRICS_CONFIG="host=https://metrics.solana.com:8086,db=tds,u=tds_writer,p=dry_run"
```

```bash
solana-validator --identity ~/validator-keypair.json --vote-account ~/vote-account-keypair.json \
    --ledger ~/validator-ledger --rpc-port 8899 --entrypoint tds.solana.com:8001 \
    --limit-ledger-size
```

To force validator logging to the console add a `--log -` argument, otherwise the validator will automatically log to a file.

Confirm your validator connected to the network by running:

```bash
solana-gossip spy --entrypoint tds.solana.com:8001
```

This command will display all the nodes that are visible to the TdS network’s entrypoint. If your validator is connected, its public key and IP address will appear in the list.
