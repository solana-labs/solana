---
title: Setup a Solana RPC Node
sidebar_label: Setup an RPC Node
---

Since an RPC server runs the same process as a consensus validator, you can follow the instructions in the [previous section](/validator-setup/initial-validator-setup) to get started. After you validator is running, you can refer to this section for RPC specific setup instructions.

## Sample RPC validator.sh

Here is an example `validator.sh` file for a testnet rpc server. You will want to be aware of the following flags:

- `--full-rpc-api`: enables all rpc operations on this validator.
- `--no-voting`: runs the validator without participating in consensus. Typically you do not want to run a validator as both a consensus node and a full rpc node due to resource constraints.
- `--private-rpc`: does not publish the validator's open rpc port in the `solana gossip` command

For more explanation on the flags used in the command, refer to `solana-validator --help`. Additionally, examples for different clusters can be found at the [Solana clusters doc](https://docs.solana.com/clusters) page. You will need to customize these commands for your use case.

```
#!/bin/bash
exec solana-validator \
    --identity /home/sol/validator-keypair.json \
    --known-validator 5D1fNXzvv5NjV1ysLjirC4WY92RNsVH18vjmcszZd8on \
    --known-validator dDzy5SR3AXdYWVqbDEkVFdvSPCtS9ihF5kJkHCtXoFs \
    --known-validator eoKpUABi59aT4rR9HGS3LcMecfut9x7zJyodWWP43YQ \
    --known-validator 7XSY3MrYnK8vq693Rju17bbPkCN3Z7KvvfvJx4kdrsSY \
    --known-validator Ft5fbkqNa76vnsjYNwjDZUXoTWpP7VYm3mtsaQckQADN \
    --known-validator 9QxCLckBiJc783jnMvXZubK4wH86Eqqvashtrwvcsgkv \
    --only-known-rpc \
    --full-rpc-api \
    --no-voting \
    --ledger /mnt/ledger \
    --accounts /mnt/accounts \
    --log /home/sol/solana-rpc.log \
    --rpc-port 8899 \
    --rpc-bind-address 0.0.0.0 \
    --private-rpc \
    --dynamic-port-range 8000-8020 \
    --entrypoint entrypoint.testnet.solana.com:8001 \
    --entrypoint entrypoint2.testnet.solana.com:8001 \
    --entrypoint entrypoint3.testnet.solana.com:8001 \
    --expected-genesis-hash 4uhcVJyU9pJkvQyS88uRDiswHXSCkY3zQawwpjk2NsNY \
    --wal-recovery-mode skip_any_corrupted_record \
    --limit-ledger-size
```

### Solana Bigtable

The Solana blockchain is able to create many transactions per second. Because of the volume of transactions on the chain, it is not practical for an RPC node to store all of the blockchain on the machine. Instead, RPC operators use the `--limit-ledger-size` flag to specify how many blocks to store on the RPC node. If the user of the RPC node needs historical blockchain data then the RPC server will have to access older blocks through a Solana bigtable instance. If you are interested in setting up your own bigtable instance, see [these docs](https://github.com/solana-labs/solana-bigtable) in the solana github repository.
