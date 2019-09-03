FROM debian:buster

# JSON RPC port
EXPOSE 8899/tcp

RUN apt update && \
    apt-get install -y bzip2 libssl-dev && \
    rm -rf /var/lib/apt/lists/*

COPY usr/bin /usr/bin/
ENTRYPOINT [ "/usr/bin/solana-run.sh" ]
CMD [""]
