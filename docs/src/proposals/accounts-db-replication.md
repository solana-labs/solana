---
title: AccountsDB Replication for RPC Services
---

## Problem

Validators fall behind the network when bogged down by heavy RPC load. This
seems to be due to a combination of CPU load and lock contention caused by
serving RPC requests. The most expensive RPC requests involve account scans.

## Solution Overview

AccountsDB replicas that run separately from the main validator can be used to
offload account-scan requests. Replicas would only: request and pull account
updates from the validator, serve account-state RPC requests, and manage
AccountsDb and AccountsBackgroundService clean + shrink.

The replica communicates to the main validator via RPC mechanism to fetch
metadata information about the replication and the accounts update from the validator.
The main validator supports only one replica node. A replica node can relay the
information to 1 or more other replicas forming a replication tree.

At the initial start of the replica node, it downloads the latest snapspshot
from a validator and constructs the bank and AccountsDb from it. After that, it queries
its main validator for new slots and request the validator to send it the updated
accounts for that slot and update to its own AccountsDb.

The same RPC replication mechansim can be used between a replica to another replica.
This requires the replica to serve both the client and server in the replication model.

### AccountsDB Modifications

### Interface
