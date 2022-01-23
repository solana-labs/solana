---
title: Standalone TPU node
---

## Problem

A Solana validator deployment processes all peer-to-peer traffic in the same process.
This limits the available bandwidth of a validator to that of a single server.

Network interfaces and upstream routers are forced to drop packets when the bandwidth of a validator is exhausted.
This has been observed during large surges in traffic to the TPU port.

Generic networking setups treat all Solana traffic equal and will randomly drop
both consensus-relevant traffic (TVU, TPUvote) and TPU traffic.
The resulting drops in TPUvote and TVU packets result in cluster instability through increased forking.

*Network Diagram: Validator Route*

```
        +---------+         +--------+        +-----------+
        |         |         |        |  (!!)  |           |
--------| Routers |---------| Switch |--------| Validator |
200G RX |         | 40G RX  |        | 1G RX  |           |
        +---------+         +--------+        +-----------+
```

Deploying QoS settings at upstream routers to prioritize consensus-relevant traffic
is only an option for validator operators that have control of datacenter network infrastructure.

## Proposed Solution

### TPU node

TPU traffic should be processed in a way that cannot affect other validator services.

An effective way is to accept TPU on another physical host than the validator itself
which is trivially done by advertising a different TPU IP address through gossip.
TPU nodes periodically call out to the validator request gossip registration.

*Network Diagram: TPU node route*

```
        +---------+         +--------+        +-----------+
        |         |         |        |        |           |
--------| Routers |---------| Switch |--------| Validator |
200G RX |         | 40G RX  |        | 1G RX  |           |
        +---------+         +--------+        +-----------+
                                 |
                                 |
                                 |            +-----------+
                                 |      (!!)  |           |
                                 +------------| TPU node  |
                                       1G RX  |           |
                                              +-----------+
```

### TPU proxy protocol

The TPU node needs to forward incoming transactions back to the validator for banking.

It is vital that the TPU node only forwards as many transactions as the validator can process.
We reintroduce the _SigVerify_ stage to allow the TPU node to drop duplicate and invalid/unsanitized transactions.
This also offloads signature verification from the validator.

These design constraints require a new _TPUProxy_ protocol with inbuilt congestion control and authentication.
It exposes a privileged API that allows registration of TPU endpoints and direct submission of transactions to the banking stage.

The final stage of the TPU node is _Proxy_ which implements the _TPUProxy_ sender.
The validator receives and processes _TPUProxy_ connections via the _ProxyFetch_ stage.

Validators accept multiple _TPUProxy_ connections to allow for round-robin or ECMP load balancing in the future.

The _ProxyFetch_ recipient should continuously monitor available system resources
and respond to downstream (banking stage) pressure by throttling the rates of the incoming transactions streams.
The _Proxy_ stage sender must drop surplus transactions once its network buffers are filled.

```
     +---------+                                 +------------+
     | TPU     |                                 | TPUvote    |
     | Clients |                                 | TPUforward |
     +----+----+                                 | Clients    |
          |                                      +-----+------+
          |                                            |
          |                                            |
+---------v---------+       +--------------------------v-----------+
|      TPU node     |       |                       Validator      |
|                   |       |                                      |
|   +-----------+   |       |                      +-----------+   |
|   | Fetch     |   |       |                      | Fetch     |   |
|   +-----+-----+   |       |                      +-----+-----+   |
|         |         |       |                            |         |
|         |         |       |                            |         |
|   +-----v-----+   |       |   +------------+     +-----v-----+   |
|   | SigVerify |   |   +-------> ProxyFetch |     | SigVerify |   |
|   +-----+-----+   |   |   |   +-----+------+     +-----+-----+   |
|         |         |   |   |         |                  |         |
|         |         |   |   |         |                  |         |
|   +-----v-----+   |   |   |         |            +-----v-----+   |
|   | Proxy     +-------+   |         +------------> Banking   |   |
|   +-----------+   |       |                      +-----+-----+   |
|                   |       |                            |         |
+-------------------+       |                            |         |
                            |                            v         |
                            |                           ...        |
                            |                                      |
                            +--------------------------------------+
```

The _TPUProxy_ protocol is based on gRPC. Mutual TLS authentication is _mandatory_.

We propose the following API definition.

```proto
import "google/protobuf/duration.proto";

// Validator TPUProxy API.
service TPUProxy {
    // Use this method to advertise a new TPU endpoint in gossip.
    // Clients should repeatedly invoke this endpoint before their TPU registration expires
    // or when a transaction stream terminates.
    rpc register_tpu_endpoint(RegisterTPURequest) returns (RegisterTPUResponse);

    // Use this method to push a stream of transactions to the validator.
    rpc stream_transactions(stream Transaction) returns (StreamTransactionsResponse);
}

message StreamTransactionsResponse {}

message RegisterTPURequest {
    string ip_address = 1;
    uint32 port = 2;
}

message RegisterTPUResponse {
    // The duration for which the TPU registration is valid.
    google.protobuf.Duration registration_ttl = 1;
}
```

### TPU failover

When the _ProxyFetch_ stage is actively processing transactions from TPU nodes,
the validator _Fetch_ will cease processing TPU traffic and remove its registration from gossip.
This effectively re-routes all TPU traffic away from the validator.

If all external TPU recipients fail however, the validator system would stop processing transactions entirely.
The system should naturally recover in this scenario by reactivating TPU processing in the validator _Fetch_ stage.

This failover is triggered when either of the following conditions are met for 10 seconds.
- No active TPU registrations (via `register_tpu_endpoint`).
- No transactions received in the last 30 seconds.

## Considerations

### TPUProxy head-of-line blocking

The _TPUProxy_ gRPC transport is based on TCP which is susceptible to head-of-line blocking through packet loss.

Some other alternatives for proxy protocols have been suggested, namely:
- QUIC streams (UDP)
- DTLS with custom congestion control (UDP)
- Regular TPU stream

gRPC was chosen as the preferred transport based on the following rationale:
- The recipient (validator) must be able to throttle senders through congestion control.
- Offloading SigVerify requires authentication (TLS).
- Widely-adopted and open standards are preferred to maximize compatibility.
  (e.g. allowing compatibility with MEV bots written in other programming languages).

### Deployment overhead

TPUProxy requires procuring additional physical infrastructure which might not be an option for some validators.

Failure to process incoming consensus-relevant traffic is suspected
to have an indirect effect of the affected validator's vote failure rate.
This might not be a sufficient incentive for validators to deploy TPU nodes however.

### DDoS protection

Deploying TPU nodes does not protect against packet flood attacks against validators since
attackers could choose to spam the validator's consensus ports instead.

Adequate DDoS protection can only be implemented at upstream network infrastructure.

## References

- [Allow arbitrary TPU address](https://github.com/solana-labs/solana/pull/22677) ([@neodyme-labs](https://github.com/neodyme-labs))
- [tpuproxy](https://github.com/certusone/tpuproxy) ([@CertusOne](https://github.com/certusone/tpuproxy))
