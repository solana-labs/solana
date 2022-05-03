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
a bound, then the runtime halts the program and returns an error.

Note: The compute budget currently applies per-instruction but is moving toward
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

## Transaction-wide Compute Budget

Transactions are processed as a single entity and are the primary unit of block
scheduling. In order to facilitate better block scheduling and account for the
computational cost of each transaction, the compute budget is moving to a
transaction-wide budget rather than per-instruction.

For information on what the compute budget is and how it is applied see [Compute
Budget](#compute-budget).

With a transaction-wide compute budget the `max_units` cap is applied to the
entire transaction rather than to each instruction within the transaction. The
transaction-wide default maximum number of units will be calculated as the product
of the instruction count and the existing per-instruction maximum units. This means
that the sum of the compute units used by each instruction in the transaction must
not exceed that value. This default maximum value attempts to retain existing behavior
to avoid breaking client logic.

### Reduce transaction fees

_Note: At the time of writing, transaction fees are still charged by the number of
signatures but will eventually be calculated from requested compute unit cap._

Most transactions won't use the default number of compute units so they can include a
[``ComputeBudgetInstruction`](https://github.com/solana-labs/solana/blob/db32549c00a1b5370fcaf128981ad3323bbd9570/sdk/src/compute_budget.rs#L39)
to lower the compute unit cap. **Important: Lower compute caps will be charged lower fees.**

Compute Budget instructions don't require any accounts and must lie in the first
3 instructions of a transaction otherwise they will be ignored.

The `ComputeBudgetInstruction::request_units` function can be used to create
these instructions:

```rust
let instruction = ComputeBudgetInstruction::request_units(300_000);
```

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
