## Minimal Safecoin Docker image
This image is automatically updated by CI

https://hub.docker.com/r/safecoinlabs/safecoin/

### Usage:
Run the latest beta image:
```bash
$ docker run --rm -p 8899:8899 --ulimit nofile=700000 safecoinlabs/safecoin:beta
```

Run the latest edge image:
```bash
$ docker run --rm -p 8899:8899 --ulimit nofile=700000 safecoinlabs/safecoin:edge
```

Port *8899* is the JSON RPC port, which is used by clients to communicate with the network.
