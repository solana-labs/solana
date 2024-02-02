---
title: Solana Commitment Status
sidebar_label: Commitment Status
pagination_label: Consensus Commitment Status
description:
  "Processed, confirmed, and finalized. Learn the differences between the
  different commitment statuses on the Solana blockchain."
keywords:
  - processed
  - confirmed
  - finalized
  - stake level
  - block
  - blockhash
---

The [commitment](https://solana.com/docs/terminology#commitment) metric gives
clients a standard measure of the network confirmation for the block. Clients
can then use this information to derive their own measures of commitment.

There are three specific commitment statuses:

- Processed
- Confirmed
- Finalized

| Property                              | Processed | Confirmed | Finalized |
| ------------------------------------- | --------- | --------- | --------- |
| Received block                        | X         | X         | X         |
| Block on majority fork                | X         | X         | X         |
| Block contains target tx              | X         | X         | X         |
| 66%+ stake voted on block             | -         | X         | X         |
| 31+ confirmed blocks built atop block | -         | -         | X         |
