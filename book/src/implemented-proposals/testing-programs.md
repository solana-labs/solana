# Testing Programs

Applications send transactions to a Solana cluster and query validators to confirm the transactions were processed and to check each transaction's result. When the cluster doesn't behave as anticipated, it could be for a number of reasons:

* The program is buggy
* The BPF loader rejected an unsafe program instruction
* The transaction was too big
* The transaction was invalid
* The Runtime tried to execute the transaction when another one was accessing

  the same account

* The network dropped the transaction
* The cluster rolled back the ledger
* A validator responded to queries maliciously

## The AsyncClient and SyncClient Traits

To troubleshoot, the application should retarget a lower-level component, where fewer errors are possible. Retargeting can be done with different implementations of the AsyncClient and SyncClient traits.

Components implement the following primary methods:

```text
trait AsyncClient {
    fn async_send_transaction(&self, transaction: Transaction) -> io::Result<Signature>;
}

trait SyncClient {
    fn get_signature_status(&self, signature: &Signature) -> Result<Option<transaction::Result<()>>>;
}
```

Users send transactions and asynchrounously and synchrounously await results.

### ThinClient for Clusters

The highest level implementation, ThinClient, targets a Solana cluster, which may be a deployed testnet or a local cluster running on a development machine.

### TpuClient for the TPU

The next level is the TPU implementation, which is not yet implemented. At the TPU level, the application sends transactions over Rust channels, where there can be no surprises from network queues or dropped packets. The TPU implements all "normal" transaction errors. It does signature verification, may report account-in-use errors, and otherwise results in the ledger, complete with proof of history hashes.

## Low-level testing

### BankClient for the Bank

Below the TPU level is the Bank. The Bank doesn't do signature verification or generate a ledger. The Bank is a convenient layer at which to test new on-chain programs. It allows developers to toggle between native program implementations and BPF-compiled variants. No need for the Transact trait here. The Bank's API is synchronous.

## Unit-testing with the Runtime

Below the Bank is the Runtime. The Runtime is the ideal test environment for unit-testing. By statically linking the Runtime into a native program implementation, the developer gains the shortest possible edit-compile-run loop. Without any dynamic linking, stack traces include debug symbols and program errors are straightforward to troubleshoot.

