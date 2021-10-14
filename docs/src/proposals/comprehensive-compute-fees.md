---
title: Comprehensive Compute Fees
---

## Motivation

The current fee structure lacks a comprehensive account of the work required by
a validator to process a transaction.  The fee structure is only based on the
number of signatures in a transaction but is meant to account for the work that
the validator must perform to validate each transaction.  The validator performs
a lot more user-defined work than just signature verification.  Processing a
transaction typically includes signature verifications, account locking, account
loading, and instruction processing.

## Proposed Solution

The following solution does not specify what native token costs are to be
associated with the new fee structure.  Instead, it sets the criteria and
provides the knobs that a cost model can use to determine those costs.

### Fee

The goal of the fees is to cover the computation cost of processing a
transaction.  Each of the fee categories below will be represented as a compute
unit cost that, when added together, encompasses the entire cost of processing
the transaction.  By calculating the total cost of the transaction, the runtime
can charge a more representative fee and make better transaction scheduling
decisions.

A fee will be calculated based on:

1. Number of signatures
   - Fixed rate per signature
2. Number of write locks
   - Fixed rate per writable account
3. Data byte cost
   - Fixed rate per byte of the sum of the length all a transactions instruction
     datas
4. Account sizes
   - Account sizes can't be known up-front but can account for a considerable
     amount of the load the transaction incurs on the network.  The payer will
     be charged for a maximum account size (10m) upfront and refunded the
     difference after the actual account sizes are known.
5. Compute budget
   - Each transaction will be given a default transaction-wide compute budget of
     200k units with the option of requesting a larger budget via a compute
     budget instruction up to a maximum of 1m units.  This budget is used to
     limit the time it takes to process a transaction.  The compute budget
     portion of the fee will be charged up-front based on the default or
     requested amount.  After processing, the actual number of units consumed
     will be known, and the payer will be refunded the difference, so the payer
     only pays for what they used.  Builtin programs will have a fixed cost
     while BPF program's cost will be measured at runtime.
6. Precompiled programs
   - Precompiled programs are performing compute-intensive operations.  The work
     incurred by a precompiled program is predictable based on the instruction's
     data array.  Therefore a cost will be assigned per precompiled program
     based on the parsing of instruction data.  Because precompiled programs are
     processed outside of the bank, their compute cost will not be reflected in
     the compute budget and will not be used in transaction scheduling
     decisions. The methods used to determine the fixed cost of the components
     above are described in
     [#19627](https://github.com/solana-labs/solana/issues/19627)

### Cost model

The cost model is used to assess what load a transaction will incur during
in-slot processing and then make decisions on how to best schedule transaction
into batches.

The cost model's criteria are identical to the fee's criteria except for
signatures and precompiled programs.  These two costs are incurred before a
transaction is scheduled and therefore do not affect how long a transaction
takes within a slot to process.

### Cache account sizes and use them instead of the max

https://github.com/solana-labs/solana/issues/20511

### Transaction-wide compute caps

The current compute budget caps are independently applied to each instruction
within a transaction. This means the overall transaction cap varies depending on
how many instructions are in the transaction.  To more accurately schedule a
transaction, the compute budget will be applied transaction-wide.  One challenge
of the transaction-wide cap is that each instruction (program) can no longer
expect to be given an equal amount of compute units.  Each instruction will be
given the remaining units left over after processing earlier instructions.  This
will provide some additional tuning and composability challenges for developers.

### Requestable compute budget caps and heap sizes

The precompiled
[ComputeBudget](https://github.com/solana-labs/solana/blob/00929f836348d76cb3503d0ba5f76f0d275bcc66/sdk/src/compute_budget.rs#L34)
program can be used to request higher transaction-wide compute budget caps and
program heap sizes.  The requested increases will be reflected in the
transaction's fee.

### Fees for precompiled program failures

https://github.com/solana-labs/solana/issues/20481

### Rate governing

Current rate governing needs to be re-assessed.  Current fees are being rate
governed down to their minimums because the number of signatures in each slot is
far lower than the "target" signatures per slot.

Instead of using the number of signatures to rate govern, the cost model will
feed back information based on the batch/queue load it is seeing.  The fees will
sit at a target rate and only increase if the load goes above a specified but to
be determined threshold.  The governing will be applied across all the fee
criteria.

### How do clients calculate a transaction's fee

Transaction fees are currently calculated based on a fixed cost per signature
and implemented via an object in the SDK that took a transaction and returned a
cost.   This object is looked up via a blockhash and returned via RPC to
calculate the fee offline.

The comprehensive fee calculations are more sophisticated and depend on a
greater amount of information (recent program compute meter measurements).  To
move to the new fee structure the RPC APIs that return a FeeCalculator object
will be deprecated, and clients will send their transactions over RPC and be
returned the calculated fee.

Fees will no longer be calculated based on a blockhash since it is
cost-prohibitive to retain the fee cost inputs for each bank (program units,
account sizes).  And mainly, the governed fee is based on network load now, not
at some time in the past. This will mean that during offline-signing the actual
fee charged will not be known until the transaction is submitted.
