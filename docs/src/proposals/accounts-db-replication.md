---
title: AccountsDB Replication for RPC Services
---

## Problem

Validators fall behind the network when bogged down by heavy RPC load. This
seems to be due to a combination of CPU load and lock contention caused by
serving RPC requests. The most expensive RPC requests involve account scans.

## Proposed Solution

Implement accountsDB replicas that run separately from the main validator to
offload account-scan requests. Replicas would only: request and pull account
updates from the validator, serve account-state RPC requests, and manage
AccountsDb and AccountsBackgroundService clean + shrink.

### AccountsDB Modifications

### Interface
