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
git clone -b k8s-cluster-official git@github.com:gregcusack/solana.git
cd solana
```

1) Build local solana version (aka based on the current commit)
```
cargo run --bin solana-k8s --
    -n <namespace e.g. greg-test>
    --num-validators 3
    --bootstrap-image <registry>/bootstrap-<image-name>:<tag>
    --validator-image <registry>/validator-<image-name>:<tag>
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
    --deploy-method tar
    --release-channel <release-channel. e.g. v1.16.5 (must prepend with 'v')>
    --do-build
    --docker-build
    --registry <docker-registry>
    --image-name <name-of-your-image-to-build>
    --base-image <base-image-of-docker-image: default ubuntu:20.04>
    --tag <imagetag. default: latest>
```

Exmple
```
cargo run --bin solana-k8s --
    -n greg-test
    --num-validators 3
    --bootstrap-image gregcusack/bootstrap-k8s-cluster-image:v2
    --validator-image gregcusack/validator-k8s-cluster-image:v2
    --deploy-method local
    --do-build
    --docker-build
    --registry gregcusack
    --image-name k8s-cluster-image
    --base-image ubuntu:20.04
    --tag v2
```

## Metrics are now supported as of 11/14/23!! ~woo~
1) Setup metrics database:
```
cd k8s-cluster/src/scripts
./init-metrics -c <database-name> <metrics-username>
# enter password when promted
```
2) add the following to your `solana-k8s` command from above
```
--metrics-host https://internal-metrics.solana.com # need the `https://` here
--metrics-port 8086
--metrics-db <database-name>            # from (1)
--metrics-username <metrics-username>   # from (1)
--metrics-password <metrics-password>   # from (1)
```

## Deploying with client
```
cargo run --bin solana-k8s --
    -n <namespace e.g. greg-test>
    --num-validators <num-validators>
    --bootstrap-image <registry>/bootstrap-<image-name>:<tag>
    --validator-image <registry>/validator-<image-name>:<tag>
    --deploy-method local
    --docker-build
    --registry <docker-registry>
    --image-name <name-of-your-image-to-build>
    --tag <imagetag. default: latest>
    -c <num-clients>
    --client-delay-start <seconds-to-wait-after-deploying-validators-before-deploying-client>
    --client-type <client-type e.g. thin-client>
    --client-to-run <type-of-client e.g. bench-tps>
    --bench-tps-args <bench-tps-args e.g. tx_count=25000>
```

## Deploying wth multiple cluster versions
- Use the `--deployment-tag <tag>` and `--no-bootstrap` flags.

Example:
Deploying cluster 1:
```
cargo run --bin solana-k8s --
    -n greg-test
    --num-validators 3
    --bootstrap-image gregcusack/bootstrap-k8s-cluster-image:monogon
    --validator-image gregcusack/validator-k8s-cluster-image:monogon
    --deploy-method local
    --do-build
    --docker-build
    --registry gregcusack
    --image-name k8s-cluster-image
    --base-image ubuntu:20.04
    --tag monogon
    --deployment-tag v1 # notice tag here. can be any unique string
```
Deploying cluster 2:
```
cargo run --bin solana-k8s --
    -n greg-test
    --num-validators 4
    --bootstrap-image gregcusack/bootstrap-k8s-cluster-image:monogon
    --validator-image gregcusack/validator-k8s-cluster-image:monogon
    --deploy-method local
    --do-build
    --docker-build
    --registry gregcusack
    --image-name k8s-cluster-image
    --base-image ubuntu:20.04
    --tag monogon
    --deployment-tag v2 # notice tag here. can be any unique string
    --no-bootstrap # ensure we do not create a new genesis and new bootstrap validator
```

If you want to do this with `--deploy-method tar`, we an do something like:
```
cargo run --bin solana-k8s --
    -n greg-test
    --num-validators 3
    --bootstrap-image gregcusack/bootstrap-k8s-cluster-image:monogon
    --validator-image gregcusack/validator-k8s-cluster-image:monogon
    --deploy-method tar
    --release-channel v1.17.2
    --do-build
    --docker-build
    --registry gregcusack
    --image-name k8s-cluster-image
    --base-image ubuntu:20.04
    --tag monogon
    --deployment-tag v1
```

```
cargo run --bin solana-k8s --
    -n greg-test
    --num-validators 3
    --bootstrap-image gregcusack/bootstrap-k8s-cluster-image:monogon
    --validator-image gregcusack/validator-k8s-cluster-image:monogon
    --deploy-method tar
    --release-channel v1.17.3
    --do-build
    --docker-build
    --registry gregcusack
    --image-name k8s-cluster-image
    --base-image ubuntu:20.04
    --tag monogon
    --deployment-tag v2
    --no-bootstrap
```

## Deploying with a specific kube config location
```
KUBECONFIG=/path/to/my/kubeconfig cargo run --bin solana-k8s -- ....
```
e.g.
```
KUBECONFIG=/home/sol/.kube/config cargo run --bin solana-k8s -- ....
```

## Verifying Deployment
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

## Trouble Shooting
1) The default `solana-validator` command includes `--require-tower`, as a result if your pods restart for whatever reason after the validator was running for some time, the validator will not properly boot. Deploy your experiment with `--skip-require-tower`

### Notes
- Have tested deployments of up to 1200 validators
- Once again, we assume you are logged into docker and you are pulling from a public repo (Monogon hosts need to access)

## TODO:
- Big one here is that we rely on the local solana-cli on your host machine to create genesis currently. as a result, the local cli needs to be compatible
with the validator version you are deploying in monogon. This will be fixed in the future
