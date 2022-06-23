---
title: "Runtime"
---

## Capability of Programs

The runtime only permits the owner program to debit the account or modify its
data. The program then defines additional rules for whether the client can
modify accounts it owns. In the case of the System program, it allows users to
transfer lamports by recognizing transaction signatures. If it sees the client
signed the transaction using the keypair's _private key_, it knows the client
authorized the token transfer.

In other words, the entire set of accounts owned by a given program can be
regarded as a key-value store, where a key is the account address and value is
program-specific arbitrary binary data. A program author can decide how to
manage the program's whole state, possibly as many accounts.

After the runtime executes each of the transaction's instructions, it uses the
account metadata to verify that the access policy was not violated. If a program
violates the policy, the runtime discards all account changes made by all
instructions in the transaction, and marks the transaction as failed.

### Policy

After a program has processed an instruction, the runtime verifies that the
program only performed operations it was permitted to, and that the results
adhere to the runtime policy.

The policy is as follows:

- Only the owner of the account may change owner.
  - And only if the account is writable.
  - And only if the account is not executable.
  - And only if the data is zero-initialized or empty.
- An account not assigned to the program cannot have its balance decrease.
- The balance of read-only and executable accounts may not change.
- Only the system program can change the size of the data and only if the system
  program owns the account.
- Only the owner may change account data.
  - And if the account is writable.
  - And if the account is not executable.
- Executable is one-way (false->true) and only the account owner may set it.
- No one can make modifications to the rent_epoch associated with this account.

## Compute Budget

To prevent a program from abusing computation resources, each instruction in a
transaction is given a compute budget. The budget consists of computation units
that are consumed as the program performs various operations and bounds that the
program may not exceed. When the program consumes its entire budget or exceeds
a bound, the runtime halts the program and returns an error.

Note: The compute budget currently applies per-instruction, but is moving toward
a per-transaction model. For more information see [Transaction-wide Compute
Budget](#transaction-wide-compute-budget).

The following operations incur a compute cost:

- Executing BPF instructions
- Calling system calls
  - logging
  - creating program addresses
  - cross-program invocations
  - ...

For cross-program invocations, the programs invoked inherit the budget of their
parent. If an invoked program consumes the budget or exceeds a bound, the entire
invocation chain and the parent are halted.

The current [compute
budget](https://github.com/solana-labs/solana/blob/db32549c00a1b5370fcaf128981ad3323bbd9570/program-runtime/src/compute_budget.rs)
can be found in the Solana Program Runtime.

For example, if the current budget is:

```rust
max_units: 200,000,
log_u64_units: 100,
create_program address units: 1500,
invoke_units: 1000,
max_invoke_depth: 4,
max_call_depth: 64,
stack_frame_size: 4096,
log_pubkey_units: 100,
...
```

Then the program

- Could execute 200,000 BPF instructions, if it does nothing else.
- Cannot exceed 4k of stack usage.
- Cannot exceed a BPF call depth of 64.
- Cannot exceed 4 levels of cross-program invocations.

Since the compute budget is consumed incrementally as the program executes, the
total budget consumption will be a combination of the various costs of the
operations it performs.

At runtime a program may log how much of the compute budget remains. See
[debugging](developing/on-chain-programs/debugging.md#monitoring-compute-budget-consumption)
for more information.

A transaction may set the maximum number of compute units it is allowed to
consume by including a "request units"
[`ComputeBudgetInstruction`](https://github.com/solana-labs/solana/blob/db32549c00a1b5370fcaf128981ad3323bbd9570/sdk/src/compute_budget.rs#L39).
Note that a transaction's prioritization fee is calculated from multiplying the
number of compute units requested by the compute unit price (measured in
micro-lamports) set by the transaction.  So transactions should request the
minimum amount of compute units required for execution to minimize fees. Also
note that fees are not adjusted when the number of requested compute units
exceeds the number of compute units consumed by an executed transaction.

Compute Budget instructions don't require any accounts and don't consume any
compute units to process.  Transactions can only contain one of each type of
compute budget instruction, duplicate types will result in an error.

The `ComputeBudgetInstruction::set_compute_unit_limit` function can be used to create
these instructions:

```rust
let instruction = ComputeBudgetInstruction::set_compute_unit_limit(300_000);
```

## Transaction-wide Compute Budget

Transactions are processed as a single entity and are the primary unit of block
scheduling. In order to facilitate better block scheduling and account for the
computational cost of each transaction, the compute budget is moving to a
transaction-wide budget rather than per-instruction.

For information on what the compute budget is and how it is applied see [Compute
Budget](#compute-budget).

The transaction-wide compute budget applies the `max_units` cap to the entire
transaction rather than to each instruction within the transaction. The default
transaction-wide `max_units` will be calculated as the product of the number of
instructions in the transaction (excluding [Compute Budget](#compute-budget)
instructions) by the default per-instruction units, which is currently 200k.
During processing, the sum of the compute units used by each instruction in the
transaction must not exceed that value. This default value attempts to retain
existing behavior to avoid breaking clients. Transactions can request a specific
number of `max_units` via [Compute Budget](#compute-budget) instructions.
Clients should request only what they need; requesting the minimum amount of
units required to process the transaction will reduce overall transaction cost,
which may include a prioritization-fee charged for every compute unit.

## New Features

As Solana evolves, new features or patches may be introduced that changes the
behavior of the cluster and how programs run. Changes in behavior must be
coordinated between the various nodes of the cluster. If nodes do not coordinate,
then these changes can result in a break-down of consensus. Solana supports a
mechanism called runtime features to facilitate the smooth adoption of changes.

Runtime features are epoch coordinated events where one or more behavior changes
to the cluster will occur. New changes to Solana that will change behavior are
wrapped with feature gates and disabled by default. The Solana tools are then
used to activate a feature, which marks it pending, once marked pending the
feature will be activated at the next epoch.

To determine which features are activated use the [Solana command-line
tools](cli/install-solana-cli-tools.md):

```bash
solana feature status
```

If you encounter problems, first ensure that the Solana tools version you are
using match the version returned by `solana cluster-version`. If they do not
match, [install the correct tool suite](cli/install-solana-cli-tools.md).
