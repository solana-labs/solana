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
regarded as a key-value store where a key is the account address and value is
program-specific arbitrary binary data. A program author can decide how to
manage the program's whole state as possibly many accounts.

After the runtime executes each of the transaction's instructions, it uses the
account metadata to verify that the access policy was not violated. If a program
violates the policy, the runtime discards all account changes made by all
instructions in the transaction and marks the transaction as failed.

### Policy

After a program has processed an instruction the runtime verifies that the
program only performed operations it was permitted to, and that the results
adhere to the runtime policy.

The policy is as follows:
- Only the owner of the account may change owner.
  - And only if the account is writable.
  - And only if the account is not executable
  - And only if the data is zero-initialized or empty.
- An account not assigned to the program cannot have its balance decrease.
- The balance of read-only and executable accounts may not change.
- Only the system program can change the size of the data and only if the system
  program owns the account.
- Only the owner may change account data.
  - And if the account is writable.
  - And if the account is not executable.
- Executable is one-way (false->true) and only the account owner may set it.
- No one modification to the rent_epoch associated with this account.

## Compute Budget

To prevent a program from abusing computation resources each instruction in a
transaction is given a compute budget.  The budget consists of computation units
that are consumed as the program performs various operations and bounds that the
program may not exceed.  When the program consumes its entire budget or exceeds
a bound then the runtime halts the program and returns an error.

The following operations incur a compute cost:
- Executing BPF instructions
- Calling system calls
  - logging
  - creating program addresses
  - cross-program invocations
  - ...

For cross-program invocations the programs invoked inherit the budget of their
parent.  If an invoked program consume the budget or exceeds a bound the entire
invocation chain and the parent are halted.

The current [compute
budget](https://github.com/solana-labs/solana/blob/d3a3a7548c857f26ec2cb10e270da72d373020ec/sdk/src/process_instruction.rs#L65)
can be found in the Solana SDK.

For example, if the current budget is:

```rust
max_units: 200,000,
log_units: 100,
log_u64_units: 100,
create_program address units: 1500,
invoke_units: 1000,
max_invoke_depth: 4,
max_call_depth: 64,
stack_frame_size: 4096,
log_pubkey_units: 100,
```

Then the program
- Could execute 200,000 BPF instructions if it does nothing else
- Could log 2,000 log messages
- Can not exceed 4k of stack usage
- Can not exceed a BPF call depth of 64
- Cannot exceed 4 levels of cross-program invocations.

Since the compute budget is consumed incrementally as the program executes the
total budget consumption will be a combination of the various costs of the
operations it performs.

At runtime a program may log how much of the compute budget remains.  See
[debugging](developing/on-chain-programs/debugging.md#monitoring-compute-budget-consumption)
for more information.

The budget values are conditional on feature enablement, take a look the compute
budget's
[new](https://github.com/solana-labs/solana/blob/d3a3a7548c857f26ec2cb10e270da72d373020ec/sdk/src/process_instruction.rs#L97)
function to find out how the budget is constructed.  An understanding of how
[features](runtime.md#features) work and what features are enabled on the
cluster being used are required to determine the current budget's values.

## New Features

As Solana evolves, new features or patches may be introduced that changes the
behavior of the cluster and how programs run.  Changes in behavior must be
coordinated between the various nodes of the cluster, if nodes do not coordinate
then these changes can result in a break-down of consensus.  Solana supports a
mechanism called runtime features to facilitate the smooth adoption of changes.

Runtime features are epoch coordinated events where one or more behavior changes
to the cluster will occur.  New changes to Solana that will change behavior are
wrapped with feature gates and disabled by default.  The Solana tools are then
used to activate a feature, which marks it pending, once marked pending the
feature will be activated at the next epoch.

To determine which features are activated use the [Solana command-line
tools](cli/install-solana-cli-tools.md):

```bash
solana feature status
```

If you encounter problems first ensure that the Solana tools version you are
using match the version returned by `solana cluster-version`.  If they do not
match [install the correct tool suite](cli/install-solana-cli-tools.md).
