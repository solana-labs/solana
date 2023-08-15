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
