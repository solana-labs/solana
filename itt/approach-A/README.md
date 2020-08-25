## **1. Approach A.**

### Steps, Commands(c<1..n>) & Outputs (o<1..n>)

c0
```bash
cd $HOME/solana/itt/approach-A
```

c1
```bash
docker build --tag itt:0.1 .
```
o1
```bash
Sending build context to Docker daemon  4.608kB
Step 1/9 : FROM rust:latest
 ---> 53f7516d0379
Step 2/9 : WORKDIR /usr/src/solana
 ---> Using cache
 ---> ba111c603655
Step 3/9 : RUN apt-get update && apt-get install -y libudev-dev apt-utils clang gcc make
 ---> Using cache
 ---> df81b10c6342
Step 4/9 : RUN git clone https://github.com/solana-labs/solana.git
 ---> Using cache
 ---> 54b65d3dc56a
Step 5/9 : RUN cd solana &&   TAG=$(git describe --tags $(git rev-list --tags --max-count=1)) &&   git checkout $TAG &&   cargo build --release
 ---> Using cache
 ---> 342d35d3f4a6
Step 6/9 : RUN cd solana &&   NDEBUG=1 ./multinode-demo/setup.sh
 ---> Using cache
 ---> b2df59ba1db4
Step 7/9 : COPY solana_itt_script.sh solana/solana_itt_script.sh
 ---> 0c4858ed9fee
Step 8/9 : WORKDIR solana
 ---> Running in 43b9cec92b81
Removing intermediate container 43b9cec92b81
 ---> fdc220b7c843
Step 9/9 : CMD ./solana_itt_script.sh
 ---> Running in 4af49cdb8ada
Removing intermediate container 4af49cdb8ada
 ---> 95f61620f037
Successfully built 95f61620f037
Successfully tagged itt:0.1
```

c2: list docker images
```bash
docker images
```
o2
```
REPOSITORY          TAG                 IMAGE ID            CREATED             SIZE
itt                 0.1                 fdb5f7f86639        5 minutes ago       4.34GB
rust                latest              53f7516d0379        2 weeks ago         1.16GB
```

c3: run docker image
```bash
docker run -it -d --rm --name itt itt:0.1
```

Now that the solana-faucet and solana-single-node-validator is running inside a docker, let's run solana-bench-tps

Open a new terminal and let's login to the running docker

c4: login into running solana docker
```bash
docker run -it -rm --name itt-bench itt:0.1 /bin/bash
```

**Some Docker commands**

```bash
docker ps
docker rm -f <CONTAINER ID>
docker images
docker rmi -f <IMAGE ID>
docker
```
