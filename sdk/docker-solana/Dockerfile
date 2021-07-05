FROM debian:buster

# RPC JSON
EXPOSE 8328/tcp
# RPC pubsub
EXPOSE 8329/tcp
# entrypoint
EXPOSE 10015/tcp
# (future) bank service
EXPOSE 8901/tcp
# bank service
EXPOSE 8902/tcp
# faucet
EXPOSE 9900/tcp
# tvu
EXPOSE 8000/udp
# gossip
EXPOSE 10015/udp
# tvu_forwards
EXPOSE 8002/udp
# tpu
EXPOSE 8003/udp
# tpu_forwards
EXPOSE 8004/udp
# retransmit
EXPOSE 8005/udp
# repair
EXPOSE 8006/udp
# serve_repair
EXPOSE 8007/udp
# broadcast
EXPOSE 8008/udp

RUN apt update && \
    apt-get install -y bzip2 libssl-dev && \
    rm -rf /var/lib/apt/lists/*

COPY usr/bin /usr/bin/
ENTRYPOINT [ "/usr/bin/solana-run.sh" ]
CMD [""]
