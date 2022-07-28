# Spin up Gossip Nodes

## Write accounts with stake to config yaml file.
#### Note: Currently stake is set to default node stake: 10000000000

```
cargo run --bin gossip-only -- --account-file <output-yaml-file> --write-keys --num-keys <number-of-keys>
```

Example:
Generate 100 keys and stakes and write to `accounts.yaml` file
```
cargo run --bin gossip-only -- --account-file accounts.yaml --write-keys --num-keys 100
```

## Spin up Gossip Nodes
#### Note: Reads in <N> keys from configuration yaml file

```
cargo run --bin gossip-only -- --account-file <input-yaml-file> --num-nodes <number of nodes> --entrypoint <IP>:<port>
```

Example:
Spin up 10 gossip nodes with a gossip entrypoint of 127.0.0.1:8001
```
cargo run --bin gossip-only -- --account-file accounts.yaml --num-nodes 10 --entrypoint 127.0.0.1:8001
```
## Running on separate nodes



### Node 1 (bootstrap node):
```
cargo run --bin gossip-only -- --account-file accounts.yaml --num-nodes 1 --entrypoint <my-vnet-ip>:<vnet-port-to-run-on> --bootstrap --gossip-host <my-vnet-ip>
```

Example:
```
cargo run --bin gossip-only -- --account-file accounts.yaml --num-nodes 1 --entrypoint 10.138.0.141:8000 --bootstrap --gossip-host 10.138.0.141
```

### Nodes 2-N:
```
cargo run --bin gossip-only -- --account-file accounts.yaml --num-nodes 1 --entrypoint <bootstrap-vnet-ip>:<bootstrap-vnet-port> --gossip-host <mt-vnet-ip>
```

Example:
```
cargo run --bin gossip-only -- --account-file accounts.yaml --num-nodes 1 --entrypoint 10.138.0.141:8000 --gossip-host 10.138.0.133
```