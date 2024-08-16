# Solana Virtual Machine specification

# Introduction

Several components of the Solana Validator are involved in processing
a transaction (or a batch of transactions).  Collectively, the
components responsible for transaction execution are designated as
Solana Virtual Machine (SVM). SVM packaged as a stand-alone library
can be used in applications outside the Solana Validator.

This document represents the SVM specification. It covers the API
of using SVM in projects unrelated to Solana Validator and the
internal workings of the SVM, including the descriptions of the inner
data flow, data structures, and algorithms involved in the execution
of transactions. The document’s target audience includes both external
users and the developers of the SVM.

## Use cases

We envision the following applications for SVM

- **Transaction execution in Solana Validator**

    This is the primary use case for the SVM. It remains a major
    component of the Agave Validator, but with clear interface and
    isolated from dependencies on other components.

    The SVM is currently viewed as realizing two stages of the
    Transaction Engine Execution pipeline as described in Solana
    Architecture documentation
    [https://docs.solana.com/validator/runtime#execution](https://docs.solana.com/validator/runtime#execution),
    namely ‘load accounts’ and ‘execute’ stages.

- **SVM Rollups**

    Rollups that need to execute a block but don’t need the other
    components of the validator can benefit from SVM, as it can reduce
    hardware requirements and decentralize the network. This is
    especially useful for Ephemeral Rollups since the cost of compute
    will be higher as a new rollup is created for every user session
    in applications like gaming.

- **SVM Fraud Proofs for Diet Clients**

    A succinct proof of an invalid state transition by the supermajority (SIMD-65)

- **Validator Sidecar for JSON-RPC**

    The RPC needs to be separated from the validator.
    `simulateTransaction` requires replaying the transactions and
    accessing necessary account data.

- **SVM-based Avalanche subnet**

    The SVM would need to be isolated to run within a subnet since the
    consensus and networking functionality would rely on Avalanche
    modules.

- **Modified SVM (SVM+)**

    An SVM type with all the current functionality and extended
    instructions for custom use cases. This would form a superset of
    the current SVM.

# System Context

In this section, SVM is represented as a single entity. We describe its
interfaces to the parts of the Solana Validator external to SVM.

In the context of Solana Validator, the main entity external to SVM is
bank. It creates an SVM, submits transactions for execution and
receives results of transaction execution from SVM.

![context diagram](/svm/doc/diagrams/context.svg "System Context")

## Interfaces

In this section, we describe the API of using the SVM both in Solana
Validator and in third-party applications.

The interface to SVM is represented by the
`transaction_processor::TransactionBatchProcessor` struct.  To create
a `TransactionBatchProcessor` object the client need to specify the
`slot`, `epoch`, and `program_cache`.

- `slot: Slot` is a u64 value representing the ordinal number of a
    particular blockchain state in context of which the transactions
    are executed. This value is used to locate the on-chain program
    versions used in the transaction execution.
- `epoch: Epoch` is a u64 value representing the ordinal number of
    a Solana epoch, in which the slot was created. This is another
    index used to locate the onchain programs used in the execution of
    transactions in the batch.
- `program_cache: Arc<RwLock<ProgramCache<FG>>>` is a reference to
    a ProgramCache instance. All on chain programs used in transaction
    batch execution are loaded from the program cache.

In addition, `TransactionBatchProcessor` needs an instance of
`SysvarCache` and a set of pubkeys of builtin program IDs.

The main entry point to the SVM is the method
`load_and_execute_sanitized_transactions`.

The method `load_and_execute_sanitized_transactions` takes the
following arguments:

- `callbacks`: A `TransactionProcessingCallback` trait instance which allows
  the transaction processor to summon information about accounts, most
  importantly loading them for transaction execution.
- `sanitized_txs`: A slice of sanitized transactions.
- `check_results`: A mutable slice of transaction check results.
- `environment`: The runtime environment for transaction batch processing.
- `config`: Configurations for customizing transaction processing behavior.

The method returns a `LoadAndExecuteSanitizedTransactionsOutput`, which is
defined below in more detail.

An integration test `svm_integration` contains an example of
instantiating `TransactionBatchProcessor` and calling its method
`load_and_execute_sanitized_transactions`.

### `TransactionProcessingCallback`

Downstream consumers of the SVM must implement the
`TransactionProcessingCallback` trait in order to provide the transaction
processor with the ability to load accounts and retrieve other account-related
information.

```rust
pub trait TransactionProcessingCallback {
    fn account_matches_owners(&self, account: &Pubkey, owners: &[Pubkey]) -> Option<usize>;

    fn get_account_shared_data(&self, pubkey: &Pubkey) -> Option<AccountSharedData>;

    fn add_builtin_account(&self, _name: &str, _program_id: &Pubkey) {}
}
```

Consumers can customize this plug-in to use their own Solana account source,
caching, and more.

### `SVMTransaction`

An SVM transaction is a transaction that has undergone the
various checks required to evaluate a transaction against the Solana protocol
ruleset. Some of these rules include signature verification and validation
of account indices (`num_readonly_signers`, etc.).

A `SVMTransaction` is a trait that can access:

- `signatures`: the hash of the transaction message encrypted using
  the signing key (for each signer in the transaction).
- `static_account_keys`: Slice of `Pubkey` of accounts used in the transaction.
- `account_keys`: Pubkeys of all accounts used in the transaction, including
  those from address table lookups.
- `recent_blockhash`: Hash of a recent block.
- `instructions_iter`: An iterator over the transaction's instructions.
- `message_address_table_lookups`: An iterator over the transaction's
  address table lookups. These are only used in V0 transactions, for legacy
  transactions the iterator is empty.

### `TransactionCheckResult`

Simply stores details about a transaction, including whether or not it contains
a nonce, the nonce it contains (if applicable), and the lamports per signature
to charge for fees.

### `TransactionProcessingEnvironment`

The transaction processor requires consumers to provide values describing
the runtime environment to use for processing transactions.

- `blockhash`: The blockhash to use for the transaction batch.
- `epoch_total_stake`: The total stake for the current epoch.
- `epoch_vote_accounts`: The vote accounts for the current epoch.
- `feature_set`: Runtime feature set to use for the transaction batch.
- `fee_structure`: Fee structure to use for assessing transaction fees.
- `lamports_per_signature`: Lamports per signature to charge per transaction.
- `rent_collector`: Rent collector to use for the transaction batch.

### `TransactionProcessingConfig`

Consumers can provide various configurations to adjust the default behavior of
the transaction processor.

- `account_overrides`: Encapsulates overridden accounts, typically used for
  transaction simulation.
- `compute_budget`: The compute budget to use for transaction execution.
- `check_program_modification_slot`: Whether or not to check a program's
  modification slot when replenishing a program cache instance.
- `log_messages_bytes_limit`: The maximum number of bytes that log messages can
  consume.
- `limit_to_load_programs`: Whether to limit the number of programs loaded for
  the transaction batch.
- `recording_config`: Recording capabilities for transaction execution.
- `transaction_account_lock_limit`: The max number of accounts that a
  transaction may lock.

### `LoadAndExecuteSanitizedTransactionsOutput`

The output of the transaction batch processor's
`load_and_execute_sanitized_transactions` method.

- `error_metrics`: Error metrics for transactions that were processed.
- `execute_timings`: Timings for transaction batch execution.
- `processing_results`: Vector of results indicating whether a transaction was
  processed or could not be processed for some reason. Note that processed
  transactions can still have failed!

# Functional Model

In this section, we describe the functionality (logic) of the SVM in
terms of its components, relationships among components, and their
interactions.

On a high level the control flow of SVM consists of loading program
accounts, checking and verifying the loaded accounts, creating
invocation context and invoking RBPF on programs implementing the
instructions of a transaction. The SVM needs to have access to an account
database, and a sysvar cache via traits implemented for the corresponding
objects passed to it. The results of transaction execution are
consumed by bank in Solana Validator use case. However, bank structure
should not be part of the SVM.

In bank context `load_and_execute_sanitized_transactions` is called from
`simulate_transaction` where a single transaction is executed, and
from `load_execute_and_commit_transactions` which receives a batch of
transactions from its caller.

Steps of `load_and_execute_sanitized_transactions`

1. Steps of preparation for execution
   - filter executable program accounts and build program accounts map (explain)
   - add builtin programs to program accounts map
   - replenish program cache using the program accounts map
        - Gather all required programs to load from the cache.
        - Lock the global program cache and initialize the local program cache.
        - Perform loading tasks to load all required programs from the cache,
          loading, verifying, and compiling (where necessary) each program.
        - A helper module - `program_loader` - provides utilities for loading
          programs from on-chain, namely `load_program_with_pubkey`.
        - Return the replenished local program cache.

2. Load accounts (call to `load_accounts` function)
   - For each `SVMTransaction` and `TransactionCheckResult`, we:
        - Calculate the number of signatures in transaction and its cost.
        - Call `load_transaction_accounts`
            - The function is interwined with the struct `SVMInstruction`
            - Load accounts from accounts DB
            - Extract data from accounts
            - Verify if we've reached the maximum account data size
            - Validate the fee payer and the loaded accounts
            - Validate the programs accounts that have been loaded and checks if they are builtin programs.
            - Return `struct LoadedTransaction` containing the accounts (pubkey and data),
              indices to the executable accounts in `TransactionContext` (or `InstructionContext`),
              the transaction rent, and the `struct RentDebit`.
            - Generate a `RollbackAccounts` struct which holds fee-subtracted fee payer account and pre-execution nonce state used for rolling back account state on execution failure.
    - Returns `TransactionLoadedResult`, containing the `LoadTransaction` we obtained from `loaded_transaction_accounts`

3. Execute each loaded transactions
   1. Compute the sum of transaction accounts' balances. This sum is
      invariant in the transaction execution.
   2. Obtain rent state of each account before the transaction
      execution. This is later used in verifying the account state
      changes (step #7).
   3. Create a new log_collector.  `LogCollector` is defined in
      solana-program-runtime crate.
   4. Obtain last blockhash and lamports per signature. This
      information is read from blockhash_queue maintained in Bank. The
      information is taken in parameters to
      `MessageProcessor::process_message`.
   5. Make two local variables that will be used as output parameters
      of `MessageProcessor::process_message`. One will contain the
      number of executed units (the number of compute unites consumed
      in the transaction). Another is a container of `ProgramCacheForTxBatch`.
      The latter is initialized with the slot, and
      the clone of environments of `programs_loaded_for_tx_batch`
         - `programs_loaded_for_tx_batch` contains a reference to all the `ProgramCacheEntry`s
            necessary for the transaction. It maintains an `Arc` to the programs in the global
            `ProgramCacheEntrys` data structure.
      6. Call `MessageProcessor::process_message` to execute the
      transaction. `MessageProcessor` is contained in
      solana-program-runtime crate. The result of processing message
      is either `ProcessedMessageInfo` which is an i64 wrapped in a
      struct meaning the change in accounts data length, or a
      `TransactionError`, if any of instructions failed to execute
      correctly.
   7. Verify transaction accounts' `RentState` changes (`verify_changes` function)
      - If the account `RentState` pre-transaction processing is rent exempt or unitiliazed, the verification will pass.
      - If the account `RentState` pre-transaction is rent paying:
         - A transition to a state uninitialized or rent exempt post-transaction is not allowed.
         - If its size has changed or its balance has increased, it cannot remain rent paying.
   8. Extract log messages.
   9. Extract inner instructions (`Vec<Vec<InnerInstruction>>`).
   10. Extract `ExecutionRecord` components from transaction context.
   11. Check balances of accounts to match the sum of balances before
       transaction execution.
   12. Update loaded transaction accounts to new accounts.
   13. Extract changes in accounts data sizes
   14. Extract return data
   15. Return `TransactionExecutionResult` with wrapping the extracted
       information in `TransactionExecutionDetails`.

4. Prepare the results of loading and executing transactions.

   This includes the following steps for each transactions
   1. Dump flattened result to info log for an account whose pubkey is
      in the transaction's debug keys.
   2. Collect logs of the transaction execution for each executed
      transaction, unless Bank's `transaction_log_collector_config` is
      set to `None`.
   3. Finally, increment various statistical counters, and update
      timings passed as a mutable reference to
      `load_and_execute_transactions` in arguments. The counters are
      packed in the struct `LoadAndExecuteTransactionsOutput`.
