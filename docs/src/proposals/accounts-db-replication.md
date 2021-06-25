---
title: AccountsDB Replication for RPC Services
---

## Problem

Validators fall behind the network when bogged down by heavy RPC load. This
seems to be due to a combination of CPU load and lock contention caused by
serving RPC requests. The most expensive RPC requests involve account scans.

## Solution Overview

AccountsDB `replicas` that run separately from the main validator can be used to
offload account-scan requests. Replicas would only: request and pull account
updates from the validator, serve client account-state RPC requests, and manage
AccountsDb and AccountsBackgroundService clean + shrink.

The replica communicates to the main validator via a new RPC mechanism to fetch
metadata information about the replication and the accounts update from the validator.
The main validator supports only one replica node. A replica node can relay the
information to 1 or more other replicas forming a replication tree.

At the initial start of the replica node, it downloads the latest snapspshot
from a validator and constructs the bank and AccountsDb from it. After that, it queries
its main validator for new slots and request the validator to send the updated
accounts for that slot and update to its own AccountsDb.

The same RPC replication mechansim can be used between a replica to another replica.
This requires the replica to serve both the client and server in the replication model.

On the previous client RPC serving side, the interfaces and implementation will be
mostly kept intact. The `JsonRpcService` keeps obtaining the accounts information through
the `Bank` interface.

The replica will also take snapshots periodically so that it can start quickly after
a restart if the snapshot is not too old.

## Detailed Solution
The following sections provides more details of the design.

### Consistency Model
The AccountsDb information is replicated asynchronously from the main validator to the replica.
When a query against the replica's AccountsDb is made, the replica may not have the latest
information of the latest slot. In this regard, it will be eventually consistent. However, for
a particular slot, the information provided is consistent with the that of its peer validator
for commitment levels confirmed and finalized. For V1, we only support queries at these two
levels.

### Solana RPC Node
A new node named solana-rpc-node will be introduced whose main responsibility is to maintain
the AccountsDb replica. The RPC node or replica node is used interchangebly in this document.
It will be a separate exectuable from the validator.

The replica consists of the following major components.

The `ReplRpcUpdatedSlotsRequestor`, this service is responsible for peridically sending the
request `ReplRpcUpdatedSlotsRequest` to its peer validator or replica for the latest slots.
It specifies the latest slot (last_replicated_slot) for which the replica has already
fetched the accounts information for.

The `ReplRpcUpdatedSlotsServer`, this service is responsible for serving the
ReplRpcUpdatedSlotsRequest and sends the `ReplRpcUpdatedSlotsResponse` back to the requestor.
The response consists of a vector of new slots which is later than the specified
last_replicated_slot.

### AccountsDB Modifications

### Interface
