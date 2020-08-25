## Approach A

### Steps
- change your working directory
- build docker image
- list docker image
- run docker image
- check log files


### Commands & Outputs

c0
```bash
cd $HOME/solana/itt/approach-A
```

c1 build docker solana image
```bash
docker build --build-arg VAR_A=1 --tag itt:0.1 .
```
o1
```bash
Sending build context to Docker daemon  4.608kB
Step 1/13 : FROM rust:latest
 ---> 53f7516d0379
Step 2/13 : WORKDIR /usr/src/solana
 ---> Using cache
 ---> d7fa2124d08a
Step 3/13 : ARG VAR_A=1
 ---> Using cache
 ---> 71651f9275d3
Step 4/13 : ENV NDEBUG=$VAR_A
 ---> Using cache
 ---> dbd3aa643075
Step 5/13 : RUN echo "VAR_A: $VAR_A"
 ---> Using cache
 ---> 41e00c984a8c
Step 6/13 : RUN echo "NDEBUG: $NDEBUG"
 ---> Using cache
 ---> 9f48fbb4d15f
Step 7/13 : RUN apt-get update && apt-get install -y apt-utils libudev-dev clang gcc make
 ---> Using cache
 ---> d1cc78cc1b01
Step 8/13 : RUN git clone https://github.com/solana-labs/solana.git
 ---> Using cache
 ---> 39bb0c692925
Step 9/13 : RUN cd solana &&   TAG=$(git describe --tags $(git rev-list --tags --max-count=1)) &&   git checkout $TAG &&   cargo build --release
 ---> Using cache
 ---> aec985b44210
Step 10/13 : RUN cd solana &&   NDEBUG=1 ./multinode-demo/setup.sh
 ---> Using cache
 ---> f2654df636d9
Step 11/13 : COPY solana_itt_script.sh solana/solana_itt_script.sh
 ---> Using cache
 ---> fd0466c23fce
Step 12/13 : WORKDIR solana
 ---> Using cache
 ---> 4e6c07ab7eee
Step 13/13 : CMD ./solana_itt_script.sh
 ---> Using cache
 ---> f2b47aac511e
Successfully built f2b47aac511e
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
docker run -it --rm -v /tmp/solana:/tmp/solana --name itt-solana itt:0.1
```

c4: check log files
```bash
ls -abl /tmp/solana
```
o4
```
/tmp/solana
├── itt-bench-tps.log
├── itt-bootstrap-validator.log
└── itt-faucet.log

0 directories, 3 files
```


Some Docker commands
```bash
docker ps
docker rm -f <CONTAINER ID>
docker images
docker rmi -f <IMAGE ID>
docker logs
```


### Setup, Run, Complete

![setup](../../itt/images/itt-approach-A-setup.png)

![run](../../itt/images/itt-approach-A-run.png)

![setup](../../itt/images/itt-approach-A-complete.png)
