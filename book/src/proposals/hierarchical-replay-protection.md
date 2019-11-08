# Hierarchical Replay Protection

## Background

The Bank protects against transactions from being replayed with a conceptual
dequeue called BlockhashQueue. Each transaction contains a blockhash and if
it's not in the queue, it is rejected with a BlockhashNotFound error. The queue
contains the most recent `MAX_RECENT_BLOCKHASHES`, which at the time of this
writing, means a client must submit a transaction within 2 minutes of getting
the most recent blockhash.

## Problem

In some cases, two minutes isn't enough time to collect the required
transaction signatures. Consider, for example, when a transaction must be
signed by a handful of offline computers.

## Proposed Solution

Inspired by memory hierarchies, add an additional BlockhashQueue that updates
every `MAX_RECENT_BLOCKHASHES` and a new RPC API `getRecentLevel2Blockhash`.
Every `MAX_RECENT_BLOCKHASHES`, the blockhash should be pushed into level 2
*instead* of level 1. By ensuring there is no overlap, one can be certain that
a blockhash from the existing API will expire in `X` minutes and that a
blockhash from level 2 will expire in `X * MAX_RECENT_BLOCKHASHES`. In fact,
the process can be repeated until with additional levels to extend expiration
times to `X * MAX_RECENT_BLOCKHASHES^(L - 1)` where `L` is the level.

And since `X` is `S * MAX_RECENT_BLOCKHASHES`, where `S` is slot duration, then
the transaction transaction expiration time at any level can be calculated as:

```
S * MAX_RECENT_BLOCKHASHES^L
```

The memory overhead of this solution is `NUM_QUEUES * LEVELS` plus however many
signatures are permitted at each level. With just three levels and 200 hashes
per level, that's expirations of roughly 2 minutes, 6.5 hours, and 55 days.

## Attacks

### Memory Exhaustion DoS

If the number of signatures at level 1 is not limited, clients could cause
memory to be exhausted by flooding level 1. That's not an issue at level 0,
because it's inherently limited to the cluster's TPS rate.

### Level 1 Starvation

If the number of transactions are limited at level 1, one could starve clients
by immediately flooding level 1 blockhashes as soon as they are available. To
guard against that, the solution could offer a way to reserve space in the
blockhash queue (implemented as the StatusCache) with a special instruction.
When that instruction is present, the runtime would permit the same fee payer
to replace its signature with a second.
