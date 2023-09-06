# Run this thing
Login to Docker for pushing/pulling repos. we assume auth for registryies are already setup.
```
docker login
```

```
kubectl create ns greg
```
```
cargo run --bin solana-k8s -- 
    -n <namespace e.g. greg-test> 
    --num-validators 3 
    --bootstrap-container boostrap-validator 
    --bootstrap-image bootstrap-k8s-cluster-image:latest 
    --validator-container validator 
    --validator-image validator-k8s-cluster-image:latest 
    --prebuild-genesis 
    --deploy-method local 
    --do-build 
    --docker-build 
    --image-tag k8s-cluster-image:latest
```

Notes:
- Builds container and the kubernetes deployments will read from images locally
- Def some hardcoded stuff in here still 
- each Docker image that is built will have a postfix of `image-tag` variable. or `k8s-cluster-image:latest` here
- each image on build will be prepended with either `bootstrap-` or `validator-` depending on the image we're building
- the `bootstrap-image` and `validator-image` must match `bootstrap-` or `validator-` and then `--image-tag`
    - this should be fixed in the future. Shouldn't have to declare `bootstrap-image` or `validator-image` names tbh


TODO:
- we currently write binary to file for genesis, then read it back to verify it. and then read it again to convert into a base64 string and then converted into binrary and then converted into a GenesisConfig lol. So need to fix
- need to add configurable accounts/stakes/etc into genesisconfig. it's currently only the bootstrap validator that is stored in genesis
- Figure out env variables for private keys. idk how this is going to work

# Kubernetes Deployment 
1) Create your namespace!
```
kubectl create ns <your-namespace>
```
2) Launch Bootstrap validator
```
kubectl apply -f bootstrap.yaml --namespace=<your-namespace>
```

3) Launch all other validators
- wait for bootstrap to come online
- edit the `validator.yaml` file if you want to increase the number of validators you want to deploy. default: 1
```
kubectl apply -f validator.yaml --namespace=<your-namespace>
```

4) Check for successful connections
- get name of bootstrap pod
```
kubectl get pods -n <your-namespace>
```
- exec into bootstrap pod
```
kubectl exec -it <pod-name-from-above> -n <your-namespace> -- /bin/bash
```
- run following commands to ensure validator connections successful. should see all pods running:
```
solana -ul validators
solana -ul gossip
```



## TODO
- [ ] Make number of validators to deploy configurable
- [ ] Configure to be able to set any type of flags needed (see net.sh scripts for gce)
- [x] Configurable namespace -> define your own namespace and deploy into it

### docker containers for solana validators
Builds off of: https://github.com/yihau/solana-local-cluster


build containers here with:
bootstrap:
```
sudo docker build -t solana-bootstrap-validator:latest -f bootstrap-validator/Dockerfile .
```

validator:
```
sudo docker build -t solana-validator:latest -f validator/Dockerfile .
```

Run bootstrap:
```
docker run -it -d --name bootstrap --network=solana-cluster --ip=192.168.0.101 solana-bootstrap-validator:latest
```

Run validator:
```
docker run -it -d --name validator --network=solana-cluster --ip=192.168.0.102 solana-validator:latest
```
