---
title: Private P2P Network
---

This document explores creating private P2P networks which would stay in sync
with the current state of a public cluster without the need to connect directly
to the public cluster. The private network would be supported by "gateway" nodes
which would bridge public cluster data to a private network. The private network
would disseminate information similarly to public cluster mechanisms.

## Rationale
There are a large number of non-voting nodes connected directly to Solana
clusters.  These nodes are connected to the public cluster in order to stay in
sync with the public cluster to serve RPC requests or provide other services
which do not require voting. These unstaked nodes consume cluster resources by
directly connecting to the pubilc cluster without participating in
voting. Public cluster resource usage would be reduced by moving these nodes
from a direct connection to a public cluster to a private P2P network of
non-voting nodes.

## Goals
- Non-gateway private P2P nodes should have minimal (if any) direct
  communication with a public cluster.
- Private network data distribution mechanisms should share existing mechanisms
with minimal changes. Turbine, repair.
- A node operating on a private P2P network should not see a substantial impact
  to the timely receipt of data compared to an unstaked node directly connected
  to a public cluster.
- Existing data distribution mechanisms (turbine, repair) should be used to
  propagate data within the private P2P network.
- Nodes running in a private P2P network should not differ substantially from
  nodes connected to the public cluster.
- Support for private P2P clusters should be implemented such that improvements
  or new features added to the validator software for public clusters should be
  available to private P2P cluster nodes without substantial code changes.
- Gateway nodes _should not_ be distinguishable from other nodes on the public
  cluster.
- Gateway nodes _should_ be distinguishable from other nodes in the respecitive
  private P2P networks.

## Solution Overview
A private P2P network will receive data through one or more "gateway" nodes
which connect to both a public Solana cluster and a private P2P network cluster.

"gataway" nodes will be configured with additional parameters secifiying
specific address endpoints to use to support a private P2P network. These
addresses may or may not be internet routable addresses.

Private P2P nodes will connect to the private network by specify a "gateway"
node private P2P network address endpoint as the entrypoint to the private
network.

"gateway" and private network nodes should be able to discover the private
network topology through a "gateway" node private address entrypoints.

The following diagram shows a public cluster and two private P2P networks.
- The public cluster is comprised of nodes: `public-node_1..public-node_n`,
  `gateway-p2p-1_1..gateway-p2p-1_m`, `gateway-p2p-2_1`.
- Private P2P Network 1 is comprised of nodes:
  `private-node-1_1..private-node-1_k`, `gateway-p2p-1_1..gateway-p2p-1_m`.
- Private P2P Network 2 is comprised of nodes:
  `private-node-2_1..private-node-2_j`, `gateway-p2p-2_1`.

```
          Public Cluster                                    Private P2P Network 1
---------------------------------------             ---------------------------------------
|                                     |             |                                     |
|  --------------------             --------------------            --------------------  |
|  |                  |             |                  |            |                  |  |
|  | public-node_1    |             | gateway-p2p-1_1  |            | private-node-1_1 |  |
|  |                  |             |                  |            |                  |  |
|  --------------------             --------------------            --------------------  |
|                                     |             |                                     |
|  --------------------               |    ...      |                       ...           |
|  |                  |               |             |                                     |
|  | public-node_2    |             --------------------            --------------------  |
|  |                  |             |                  |            |                  |  |
|  --------------------             | gateway-p2p-1_m  |            | private_node-1_k |  |
|                                   |                  |            |                  |  |
|       ...                         --------------------            --------------------  |
|                                     |             |                                     |
|  --------------------               |             ---------------------------------------
|  |                  |               |
|  | public-node_n    |               |
|  |                  |               |                      Private P2P Network 2
|  --------------------               |             ---------------------------------------
|                                     |             |                                     |
|                                   --------------------            --------------------  |
|                                   |                  |            |                  |  |
|                                   | gateway-p2p-2_1  |            | private_node-2_1 |  |
|                                   |                  |            |                  |  |
|                                   --------------------            --------------------  |
|                                     |             |                                     |
|                                     |             |                       ...           |
|                                     |             |                                     |
|                                     |             |               --------------------  |
|                                     |             |               |                  |  |
|                                     |             |               | private_node-2_j |  |
|                                     |             |               |                  |  |
|                                     |             |               --------------------  |
|                                     |             |                                     |
---------------------------------------             ---------------------------------------
```


## Gossip
Details TBD.

## Turbine
Shreds received from turbine by gateway nodes should be rebroadcast to private
P2P network nodes using turbine.

Details TBD.

## Repair
Public cluster repair determines recipients for repair requests based on stake
weight of validator nodes.  Private P2P cluster nodes should prioritize repair
requests to "gateway" cluster nodes which are receiving data from the public
cluster.

Details TBD.

## Detailed Design
TBD

## Unknowns
