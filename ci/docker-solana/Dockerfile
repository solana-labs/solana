FROM debian:stretch
RUN apt update && \
    apt-get install -y libssl-dev && \
    rm -rf /var/lib/apt/lists/*
COPY cargo-install/bin /usr/bin/
