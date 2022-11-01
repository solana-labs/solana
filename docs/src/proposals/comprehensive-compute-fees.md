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

A fee could be calculated based on:

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
     while SBF program's cost will be measured at runtime.
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

### Requestable compute budget caps and heap sizes

The precompiled
[ComputeBudget](https://github.com/solana-labs/solana/blob/00929f836348d76cb3503d0ba5f76f0d275bcc66/sdk/src/compute_budget.rs#L34)
program can be used to request higher transaction-wide compute budget caps and
program heap sizes.  The requested increases will be reflected in the
transaction's fee.

### Fees for precompiled program failures

https://github.com/solana-labs/solana/issues/20481

### Rate governing

Current rate governing needs to be re-assessed.  Fees are being rate
governed down to their minimums because the number of signatures in each slot is
far lower than the "target" signatures per slot.

Instead of using the number of signatures to rate govern, the cost model will
feed back information based on the batch/queue load it is seeing.  The fees will
sit at a target rate and only increase if the load goes above a specified but to
be determined threshold.  The governing will be applied across all the fee
criteria.

### Deterministic fees

Solana's fees are currently deterministic based on a given blockhash.  This
determinism is a nice feature that simplifies client interactions.  An example
is when draining an account that is also the payer, the transaction issuer can
pre-compute the fee and then set the entire remaining balance to be transferred
out without worrying that the fee will change leaving a very small amount
remaining in the account.  Another example is for offline signing, the payer
signer can guarantee what fee that will be charged for the transaction based on
the nonce's blockhash.

Determinism is achieved in two ways:
- blockhash queue contains a list of recent (<=~2min) blockhashes and a
  `lamports_per_signature` value.  The blockhash queue is one of the snapshot's
  serialized members and thus bank hash depends on it.
- Nonce accounts used for offline signing contain a `lamports_per_signature`
  value in its account data

In both cases, when a transaction is assessed a fee, the
`lamports_per_signature` to use is looked up (either in the queue or in the
nonce account's data) using the transaction's blockhash.

This currently comes with the following challenges:
- Exposing the `FeeCalculator` object to the clients (holds the
  `lamports_per_signature`) makes it hard to evolve the fee criteria due to
  backward-compatibility.  This issue is being solved by deprecating the
  `FeeCalculator` object and instead the new apis take a message and return a
  fee.
- Blockhash queue entries contain the fee criteria specifics and are part of the
  bankhash so evolving the fees over time involves more work/risk
- Nonce accounts store the fee criteria directly in their account data so
  evolving the fees over time requires changes to nonce account data and data
  size.

Two solutions to the latter two challenges
- Get rid of the concept of deterministic fees.  Clients ask via RPC to
  calculate the current fee estimate and the actual fee is assessed when the
  transaction is processed.  Fee changes will be governed and change slowly
  based on network load so the fee differences will be small within the 2min
  window.  Nonce accounts no longer store the fee criteria but instead a fee
  cap.  If the assessed fee at the time of processing exceeds the cap then the
  transaction fails.  This solution removes fee criteria entirely from the
  blockhash queue and nonce accounts and removes the need for either of those to
  evolve if there is a need for fee criteria to evolve.
- Retain the concept of deterministic fees.  Clients ask via RPC to calculate
  the current fee and pass in a blockhash that fee will be associated with.
  Blockhash queue and nonce accounts switch to a versioned but internal "Fee"
  object (similar to "FeeCalculator").  Each time there is a need for fees to
  evolve the fee object will add a new version and new blockhash queue entries
  and new nonce accounts will use the new version.
