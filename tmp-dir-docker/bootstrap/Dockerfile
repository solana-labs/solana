 
FROM ubuntu:20.04             
RUN apt update
RUN apt install -y iputils-ping curl vim bzip2 

RUN useradd -ms /bin/bash solana
RUN adduser solana sudo
USER solana

COPY ./fetch-perf-libs.sh ./fetch-spl.sh ./scripts /home/solana/
COPY ./net ./multinode-demo /home/solana/
RUN mkdir -p /home/solana/.cargo/bin

COPY ./farf/bin/* /home/solana/.cargo/bin/
COPY ./farf/version.yaml /home/solana/

RUN mkdir -p /home/solana/config

WORKDIR /home/solana
