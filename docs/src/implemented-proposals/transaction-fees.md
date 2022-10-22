---
title: Deterministic Transaction Fees
---

## Calculating fees

Before sending a transaction to the cluster, a client may query the network to
determine what the transaction's fee will be via the rpc request
[getFeeForMessage](/api#getfeeformessage).

## Fee Parameters

The fee is based on the number of signatures in the transaction, the more
signatures a transaction contains, the higher the fee. In addition, a
transaction can specify an additional fee that determines how the transaction is
relatively prioritized against others. A transaction's prioritization fee is
calculated by multiplying the number of compute units by the compute unit price
(measured in micro-lamports) set by the transaction via compute budget
instructions.
