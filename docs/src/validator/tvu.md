---
title: Transaction Validation Unit in a Solana Validator
sidebar_position: 3
sidebar_label: TVU
pagination_label: Validator's Transaction Validation Unit (TVU)
---

TVU (Transaction Validation Unit) is the logic of the validator
responsible for validating and propagating blocks and processing
those blocks' transactions through the runtime.

![TVU Block Diagram](/img/tvu.svg)

## Retransmit Stage

![Retransmit Block Diagram](/img/retransmit_stage.svg)
