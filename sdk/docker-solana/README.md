## Minimal Safecoin Docker image
This image is automatically updated by CI

https://hub.docker.com/r/solanalabs/solana/

### Usage:
Run the latest beta image:
```bash
$ docker run --rm -p 8328:8328 --ulimit nofile=700000 solanalabs/solana:beta
```

Run the latest edge image:
```bash
$ docker run --rm -p 8328:8328 --ulimit nofile=700000 solanalabs/solana:edge
```

Port *8328* is the JSON RPC port, which is used by clients to communicate with the network.
