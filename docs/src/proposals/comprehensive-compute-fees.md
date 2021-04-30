---
title: Comprehensive Compute Fees
---

## Motivation

The current fee structure lacks a comprehensive account of the work required by
a validator to process a transaction.  The fee structure is only based on the
number of signatures in a transaction but is meant to account for the work that
the validator must perform to validate each transaction.  The validator performs
a lot more user-defined work than just signature verification.  The work to
process a transaction typically includes signature verifications, account
locking, account loading, and instruction processing.

## Proposed Solution

### New fee structure

In addition to signature checking, the other work required to process an
instruction should be taken into account.  To do this, the total fee should be
an accumulated cost of all the work.  Each piece of work can be
measured/accounted for in a way that makes sense for the type of work involved.

A new fee structure could include:
1. flat fee per signature
2. flat fee for each write lock
3. per-byte fee for the amount of data in each loaded account
4. per-compute-unit fee for the number of compute units used to process an
   instruction

Fees 1-3 can be determined upfront before the message is processed.  #4 would
need to be finally accounted for after the message is processed, but the payer's
balance could be pre-deducted against a compute budget cap to ensure they have
enough lamports to cover a max fee, and then any unused compute budget could be
credited back after the message is fully processed.

The goal of the fees is to cover the computation cost of processing a
transaction.  Each of the above fee categories could be represented as a compute
unit cost that, when added together, encompasses the entire cost of processing
the transaction.  By calculating the total cost of the transaction the runtime
can make better decisions on what transactions to process and when, as well as
throw out transactions that exceed a cap.

The per-compute unit fee doesn't have to be linear; developers could be
non-linearly incentivized to reduce compute costs by optimizing in any of the 4
fee categories listed above.

To give developers some control over fees and the compute cap, a new built-in
instruction could be introduced that requests a specific transaction-wide
compute budget cap.  The instruction can be used by a developer to reduce the
cap and thus fees they expect to pay, or to request a higher than the default
cap.  The runtime could in-turn, use these instructions to determine how to
schedule these more expensive transactions.

### Transaction compute caps

The current compute caps are independently applied to individual instructions.
This means the overall transaction cap varies depending on how many instructions
are in the transaction.  Instead, a transaction-wide cap is probably more
appropriate.  One challenge of the transaction-wide cap is that each instruction
(program) could no longer expect to be given an equal amount of compute units
since the number of compute units will be based on the number of units already
consumed by earlier instructions in the message.  This will provide some
additional tuning and composability challenges for developers.

## Proposed steps to implement

- Apply the compute budget cap across the entire transaction rather than
  per-instruction
- Convert the per-sig fee to a per-sig compute cost and incorporate it into the
  transaction-wide cap.  Initially, keep charging the current per-sig fee.
- Add compute costs for write locks and account data loading and incorporate
  those into the transaction-wide cap
- Remove the per-sig centric fee and start charging fees based on the
  transaction compute budget
- Rework how the bank decides which transactions to process and when based on
  the compute budget expectations
- Add a built-in instruction that requests the amount of compute budget up front

  ## Things to ponder

  - Can things like write locks be meaningfully converted into a compute
    cost/cap?
  - Should account data size be accounted for in the compute cost/cap
  - Calculating a transaction's fee upfront becomes more difficult, would
    probably have to be reworded to be max fee based on the compute cap.  Actual
    fee would be equal to or less than that.
  - "Compute cost" is a lame name, ideas for something better?