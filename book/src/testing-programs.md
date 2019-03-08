## Testing Programs

Applications send transactions to a Solana cluster and query validators to
confirm the transactions were processed and to check each transaction's result.
When the cluster doesn't behave as anticipated, it could be for a number of
reasons:

* The program is buggy
* The BPF loader rejected an unsafe program instruction
* The transaction was too big
* The transaction was invalid
* The Runtime tried to execute the transaction when another one was accessing
  the same account
* The network dropped the transaction
* The cluster rolled back the ledger
* A validator responded to queries maliciously

### The Transact Trait

To troubleshoot, the application should retarget a lower-level component, where
fewer errors are possible. Retargeting can be done with different
implementations of the Transact trait.

When Futures 0.3.0 is released, the Transact trait may look like this:

```rust,ignore
trait Transact {
    async fn send_transactions(txs: &[Transaction]) -> Vec<Result<(), BankError>>;
}
```

Users send transactions and asynchrounously await their results.

### Transact with Clusters

The highest level implementation targets a Solana cluster, which may be a
deployed testnet or a local cluster running on a development machine.

### Transact with a Fullnode

Below the cluster level is a Fullnode implementation of Transact. At the
Fullnode level, the application is still using sockets, but does not need be
concerned with cluster dynamics such as leader rotation of rollback.

### Transact with the TPU

The next level is the TPU implementation of Transact. At the TPU level, the
application sends transactions over Rust channels, where there can be no
surprises from network queues or dropped packets. The TPU implements all
"normal" transaction errors. It does signature verification, may report
account-in-use errors, and otherwise results in the ledger, complete with proof
of history hashes.

### Transact with the Bank

Below the TPU level is the Bank implementation. The Bank doesn't do signature
verification or generate a ledger. The Bank is a convenient layer at which to
test new on-chain programs. It allows developers to toggle between native
program implementations and BPF-compiled variants.

### Transact with the Runtime

Below the Runtime level is the Runtime implementation. By statically linking
the Runtime into a native program implementation, the developer gains the
shortest possible edit-compile-run loop. Without any dynamic linking, stack
traces include debug symbols and program errors are straightforward to
troubleshoot.
