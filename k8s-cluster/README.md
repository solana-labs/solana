# Build a validator docker container for deploying a set of validators in a kubernetes cluster
Use w/ https://github.com/solana-labs/k8s-cluster

# Setup
From your local build host, login to Docker for pushing/pulling repos. we assume auth for registryies are already setup.
```
docker login
```

```
kubectl create ns <namespace>
```

Clone the repo
```
git clone git@github.com:solana-labs/solana.git
cd solana
```

# Build docker container
1) Build local solana version (aka based on the current commit)
```
cargo run --bin solana-k8s --
    --deploy-method local
    --do-build
    --docker-build
    --registry <your-docker-registry-name>
    --image-name <name-of-your-image-to-build>
    --base-image <base-image-of-docker-image: default ubuntu:20.04>
    --tag <imagetag. default: latest>
```

2) Pull specific release (e.g. v1.16.5)
```
cargo run --bin solana-k8s --
    --deploy-method tar
    --release-channel <release-channel. e.g. v1.16.5 (must prepend with 'v')>
    --do-build
    --docker-build
    --registry <docker-registry>
    --image-name <name-of-your-image-to-build>
    --base-image <base-image-of-docker-image: default ubuntu:20.04>
    --tag <imagetag. default: latest>
```

Example
```
cargo run --bin solana-k8s --
    --deploy-method local
    --do-build
    --docker-build
    --registry gregcusack
    --image-name k8s-cluster-image
    --base-image ubuntu:20.04
    --tag v2
```

Notes:
- Scripts that run the actual validator code are in `src/scripts`
- Registry needs to be remotely accessible by all monogon nodes
- We use the `docker` cli here for building and pushing docker images since the `docker-rs` rust library has sparse features (doesn't support `docker push`) and is very slow building docker images