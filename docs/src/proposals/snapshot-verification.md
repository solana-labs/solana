---
title: Snapshot Verification
---

## Problem

Snapshot verification of the account states is implemented, but the bank hash of the snapshot which is used to verify is falsifiable.

## Solution

While a validator is processing transactions to catch up to the cluster from the snapshot, use incoming vote transactions and the commitment calculator to confirm that the cluster is indeed building on the snapshotted bank hash. Once a threshold commitment level is reached, accept the snapshot as valid and start voting.
