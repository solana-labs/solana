# Spin up Gossip Nodes

## Write accounts with stake to config yaml file.
#### Note: Currently stake is set to default node stake: 10000000000

```
cargo run --bin testing -- --account-file <output-yaml-file> --write-keys --num-keys <number-of-keys>
```

Example:
Generate 100 keys and stakes and write to `accounts.yaml` file
```
cargo run --bin testing -- --account-file accounts.yaml --write-keys --num-keys 100
```

## Spin up Gossip Nodes
#### Note: Reads in <N> keys from configuration yaml file

```
cargo run --bin testing -- --account-file <input-yaml-file> --num-nodes <number of nodes> --entrypoint <IP>:<port>
```

Example:
Spin up 10 gossip nodes with a gossip entrypoint of 127.0.0.1:8001
```
cargo run --bin testing -- --account-file accounts.yaml --num-nodes 10 --entrypoint 127.0.0.1:8001
```