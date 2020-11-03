---
title: "Compute budget"
---

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
[debugging](developing/deployed-programs/debugging.md#monitoring-compute-budget-consumption)
for more information.

The budget values are conditional on feature enablement, take a look the compute
budget's
[new](https://github.com/solana-labs/solana/blob/d3a3a7548c857f26ec2cb10e270da72d373020ec/sdk/src/process_instruction.rs#L97)
function to find out how the budget is constructed.  An understanding of how
[features](runtime-features.md) work and what features are enabled on the
cluster being used are required to determine the current budget's values.