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

* Shred Fetch Stage - Retreives or collects forwarded shreds from other validators. Repairs them with erasure code if corrupted.

* Shred Verify Leader Signature Stage - Verifies the authenticity of the leader's signature on the shreds. This crucial step prevents unauthorized changes to the blockchain data and confirms the block's origin.

* Retransmit Stage - Acts as a relay point for sharing validated shreds with neighboring validator nodes. It interacts with the Gossip Service to obtain **peer list**. It also uses stake information from the Bank component to prioritize forwarding shreds to validators with higher stakes among neighborhood.

* Replay Stage - Executes the transactions within the validated blocks in a sequential order to update the validator's local copy of the blockchain (the ledger). This ensures consistency with the global state of the network. The Replay stage handles the resulting changes to account balances, program state, and other critical ledger updates.

* Transaction Status Service (Optional) - Provides a convenient way for external clients (like user wallets) to query the status and outcomes of submitted transactions.

## Retransmit Stage

![Retransmit Block Diagram](/img/retransmit_stage.svg)

* Window Service - Manages incoming shreds, data structures for tracking received data, retransmission requests, and potentially repair processes.
* Blockstore - Blockstore receives shreds and sends them to Deshredder to convert into an entry for Replay stage.
* Retransmitter - Shares validated shreds with peer validators.