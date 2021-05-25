---
title: Leader-to-Leader Transition
---

This design describes how leaders transition production of the PoH ledger between each other as each leader generates its own slot.

## Challenges

Current leader and the next leader are both racing to generate the final tick for the current slot. The next leader may arrive at that slot while still processing the current leader's entries.

The ideal scenario would be that the next leader generated its own slot right after it was able to vote for the current leader. It is very likely that the next leader will arrive at their PoH slot height before the current leader finishes broadcasting the entire block.

The next leader has to make the decision of attaching its own block to the last completed block, or wait to finalize the pending block. It is possible that the next leader will produce a block that proposes that the current leader failed, even though the rest of the network observes that block succeeding.

The current leader has incentives to start its slot as early as possible to capture economic rewards. Those incentives need to be balanced by the leader's need to attach its block to a block that has the most commitment from the rest of the network.

## Leader timeout

While a leader is actively receiving entries for the previous slot, the leader can delay broadcasting the start of its block in real time. The delay is locally configurable by each leader, and can be dynamically based on the previous leader's behavior. If the previous leader's block is confirmed by the leader's TVU before the timeout, the PoH is reset to the start of the slot and this leader produces its block immediately.

The downsides:

- Leader delays its own slot, potentially allowing the next leader more time to

  catch up.

The upsides compared to guards:

- All the space in a block is used for entries.
- The timeout is not fixed.
- The timeout is local to the leader, and therefore can be clever. The leader's heuristic can take into account turbine performance.
- This design doesn't require a ledger hard fork to update.
- The previous leader can redundantly transmit the last entry in the block to the next leader, and the next leader can speculatively decide to trust it to generate its block without verification of the previous block.
- The leader can speculatively generate the last tick from the last received entry.
- The leader can speculatively process transactions and guess which ones are not going to be encoded by the previous leader. This is also a censorship attack vector. The current leader may withhold transactions that it receives from the clients so it can encode them into its own slot. Once processed, entries can be replayed into PoH quickly.

## Alternative design options

### Guard tick at the end of the slot

A leader does not produce entries in its block after the _penultimate tick_, which is the last tick before the first tick of the next slot. The network votes on the _last tick_, so the time difference between the _penultimate tick_ and the _last tick_ is the forced delay for the entire network, as well as the next leader before a new slot can be generated. The network can produce the _last tick_ from the _penultimate tick_.

If the next leader receives the _penultimate tick_ before it produces its own _first tick_, it will reset its PoH and produce the _first tick_ from the previous leader's _penultimate tick_. The rest of the network will also reset its PoH to produce the _last tick_ as the id to vote on.

The downsides:

- Every vote, and therefore confirmation, is delayed by a fixed timeout. 1 tick, or around 100ms.
- Average case confirmation time for a transaction would be at least 50ms worse.
- It is part of the ledger definition, so to change this behavior would require a hard fork.
- Not all the available space is used for entries.

The upsides compared to leader timeout:

- The next leader has received all the previous entries, so it can start processing transactions without recording them into PoH.
- The previous leader can redundantly transmit the last entry containing the _penultimate tick_ to the next leader. The next leader can speculatively generate the _last tick_ as soon as it receives the _penultimate tick_, even before verifying it.
