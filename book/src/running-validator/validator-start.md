# Starting a Validator

## Configure Solana CLI

The solana cli includes `get` and `set` configuration commands to automatically
set the `--url` argument for cli commands. For example:

```bash
solana set --url http://testnet.solana.com:8899
```

\(You can always override the set configuration by explicitly passing the
`--url` argument with a command, eg: `solana --url http://beta.testnet.solana.com:8899 balance`\)

## Confirm The Testnet Is Reachable

Before attaching a validator node, sanity check that the cluster is accessible
to your machine by fetching the transaction count:

```bash
solana get-transaction-count
```

Inspect the network explorer at
[https://explorer.solana.com/](https://explorer.solana.com/) for activity.

View the [metrics dashboard](https://metrics.solana.com:3000/d/testnet-beta/testnet-monitor-beta?var-testnet=testnet)
for more detail on cluster activity.

## Confirm your Installation

Try running following command to join the gossip network and view all the other
nodes in the cluster:

```bash
solana-gossip --entrypoint testnet.solana.com:8001 spy
# Press ^C to exit
```

## Enabling CUDA

If your machine has a GPU with CUDA installed \(Linux-only currently\), include
the `--cuda` argument to `solana-validator`.

When your validator is started look for the following log message to indicate
that CUDA is enabled: `"[<timestamp> solana::validator] CUDA is enabled"`

## Tune System

For Linux validators, the solana repo includes a daemon to adjust system settings to optimize
performance (namely by increasing the OS UDP buffer limits, and scheduling PoH with realtime policy).

The daemon (`solana-sys-tuner`) is included in the solana binary release.

To run it:

```bash
sudo solana-sys-tuner --user $(whoami) > sys-tuner.log 2>&1 &
```

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

Your validator identity keypair uniquely identifies your validator within the
network. **It is crucial to back-up this information.**

If you don’t back up this information, you WILL NOT BE ABLE TO RECOVER YOUR
VALIDATOR if you lose access to it. If this happens, YOU WILL LOSE YOUR
ALLOCATION OF LAMPORTS TOO.

To back-up your validator identify keypair, **back-up your
"validator-keypair.json” file to a secure location.**

### Vanity Keypair

You can generate a custom vanity keypair using solana-keygen. For instance:

```bash
solana-keygen grind --starts-with e1v1s
```

Depending on the string requested, it may take days to find a match...

## More Solana CLI Configuration

Now that you have a keypair, set the solana configuration to use your validator
keypair for all following commands:

```bash
solana set --keypair ~/validator-keypair.json
```

You should see the following output:

```text
Wallet Config Updated: /home/solana/.config/solana/wallet/config.yml
* url: http://testnet.solana.com:8899
* keypair: /home/solana/validator-keypair.json
```

## Airdrop & Check Validator Balance

Airdrop yourself some SOL to get started:

```bash
solana airdrop 1000
```

To view your current balance:

```text
solana balance
```

Or to see in finer detail:

```text
solana balance --lamports
```

Read more about the [difference between SOL and lamports here](../introduction.md#what-are-sols).

## Create Vote Account

If you haven’t already done so, create a vote-account keypair and create the
vote account on the network. If you have completed this step, you should see the
“validator-vote-keypair.json” in your Solana runtime directory:

```bash
solana-keygen new -o ~/validator-vote-keypair.json
```

Create your vote account on the blockchain:

```bash
solana create-vote-account ~/validator-vote-keypair.json ~/validator-keypair.json
```

## Connect Your Validator

Connect to a testnet cluster by running:

```bash
solana-validator --identity-keypair ~/validator-keypair.json --voting-keypair ~/validator-vote-keypair.json \
    --ledger ~/validator-ledger --rpc-port 8899 --entrypoint testnet.solana.com:8001 \
    --limit-ledger-size
```

To force validator logging to the console add a `--log -` argument, otherwise
the validator will automatically log to a file.

Confirm your validator connected to the network by opening a new terminal and
running:

```bash
solana-gossip spy --entrypoint testnet.solana.com:8001
```

If your validator is connected, its public key and IP address will appear in the list.

### Controlling local network port allocation

By default the validator will dynamically select available network ports in the
8000-10000 range, and may be overridden with `--dynamic-port-range`. For
example, `solana-validator --dynamic-port-range 11000-11010 ...` will restrict
the validator to ports 11000-11011.

### Limiting ledger size to conserve disk space

The `--limit-ledger-size` arg will instruct the validator to only retain the
last couple hours of ledger. To retain the full ledger, simply remove that arg.
