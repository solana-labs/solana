FROM debian:buster

# JSON RPC port
EXPOSE 8899/tcp

# Install libssl
RUN apt update && \
    apt-get install -y libssl-dev && \
    rm -rf /var/lib/apt/lists/*

COPY usr/bin /usr/bin/
ENTRYPOINT [ "/usr/bin/solana-run.sh" ]
CMD [""]
