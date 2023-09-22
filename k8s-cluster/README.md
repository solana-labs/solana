# How to run
From your local build host, login to Docker for pushing/pulling repos. we assume auth for registryies are already setup.
```
docker login
```

```
kubectl create ns <namespace>
```

Clone the repo
```
git clone -b solana-k8s-cluster git@github.com:gregcusack/solana.git
cd solana
```

1) Build local solana version (aka based on the current commit)
```
cargo run --bin solana-k8s --
    -n <namespace e.g. greg-test>
    --num-validators 3
    --bootstrap-image <registry>/bootstrap-<image-name>:<tag>
    --validator-image <registry>/validator-<image-name>:<tag>
    --prebuild-genesis
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
    -n <namespace e.g. greg-test>
    --num-validators 3
    --bootstrap-image <registry>/bootstrap-<image-name>:<tag>
    --validator-image <registry>/validator-<image-name>:<tag>
    --prebuild-genesis
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
    -n greg-test
    --num-validators 3
    --bootstrap-image gregcusack/bootstrap-k8s-cluster-image:v2
    --validator-image gregcusack/validator-k8s-cluster-image:v2
    --prebuild-genesis
    --deploy-method local
    --do-build
    --docker-build
    --registry gregcusack
    --image-name k8s-cluster-image
    --base-image ubuntu:20.04
    --tag v2
```

Verify validators have deployed:
```
kubectl get pods -n <namespace>
```
^ `STATUS` should be `Running` and `READY` should be `1/1` for all

Verify validators are connected properly:
```
BOOTSTRAP_POD=$(kubectl get pods -n greg-test | grep bootstrap | awk '{print $1}')
kubectl exec -it -n <namespace> $BOOTSTRAP_POD -- /bin/bash

solana -ul gossip # should see `--num-validators`+1 nodes (including bootstrap)
solana -ul validators # should see `--num-validators`+1 current validators (including bootstrap)
```
^ if you ran the tar deployment, you should see the Stake by Version as well read `<release-channel>` in the `solana -ul validators` output.

Important Notes: this isn't designed the best way currently.
- bootstrap and validator docker images are build exactly the same although the pods they run in are not build the same
- `--bootstrap-image` and `--validator-image` must match the format outlined above. The pods in the replicasets pull the exact `--bootstrap-image` and `--validator-image` images from dockerhub.
    - The docker container that is build is named based off of `--registry`, `--image-name`, `--base-image`, and `--tag`
    - Whenever image name you give will be prepended with either `bootstrap-` or `validator-`. not the best design currently but that's currently how it's build.
    So make sure when you set `--boostrap-image` you make sure to prepend the `image-name` with `boostrap-` and same thing for `--validator-image` w/ `validator-`

Other Notes:
- Def some hardcoded stuff in here still
- genesis creation is hardcoded for the most part in terms of stakes, lamports, etc.
- genesis includes bootstrap accounts and SPL args.
- Registry needs to be remotely accessible by all monogon nodes

TODO:
- we currently write binary to file for genesis, then read it back to verify it. and then read it again to convert into a base64 string and then converted into binrary and then converted into a GenesisConfig lol. So need to fix

### Notes
- Have tested deployments of up to 200 validators
- Additional validator commandline flags are coming....stay tuned
- Once again, we assume you are logged into docker and you are pulling from a public repo (Monogon hosts need to access)