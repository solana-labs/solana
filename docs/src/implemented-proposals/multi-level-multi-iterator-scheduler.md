---
title: Multi-Level Multi-Iterator Scheduler
---

## Background

We have proposed graph-based scheduling in the past, however, as a hold-over until we implement those the current scheduler has been modified to use a multi-iterator approach. This approach significantly improves single-threaded throughput in situations with high-contention - and lends itself to a straight-forward stepping stone to graph-based scheduling, with a few modifications.

## Single-Level Multi-Iterator

The single-level multi-iterator has already been implemented in Solana's independent banking stage thread schedulers, this section serves to outline that concept in order to provide context for the multi-level multi-iterator.

First, we take our priority-queue and collect the entire buffer into a vector. We initialize multiple, in this case 64, positions such that there are no write-conflicts between the transactions at the given positions. These, up to 64, transactions then make up a transaction batch to be executed. The multi-iterator marks these transactions as `already_processed`, and the positions are marched forward, again checking for conflicts as we set them.

### Example

For simplicity's sake, let's assume we have a batch-size of 2 instead of 64, and a priority-queue with the following transactions:

```text
[A, A, A, B, C, A, B, C, D]
 ^        ^                     // initial iterator positions
```

The first batch would be `[A, B]`. Marching the positions forward, we get:

```text
[A, A, A, B, C, A, B, C, D]
    ^        ^
```

and the second batch is `[C, D]`. Marching the positions forward again, we get:

```text
[A, A, A, B, C, A, B, C, D]
       ^           ^
```

giving a batch of `[A, B]`. Note that the second iterator skipped over an `A` transaction, as that transaction would conflict with the `A` our first position is pointing to. Following this the remaining batches would be `[[A, C], [D]]`.

## Multi-Level Multi-Iterator

The multi-iterator works well for a single thread, but we should not use it directly in a centralized scheduler. If we have two independent hot events touching accounts `A` and `B`, the single-level multi-iterator would put them into the same batch, which would then be serialized. This is not ideal, as we would like to be able to execute these transactions in parallel.

The multi-level multi-iterator uses multiple multi-iterators to construct the batches for our threads, while maintaining that all transactions can be executed in parallel, and that the hottest events would get scheduled to separate threads.

If we have 4 threads and a batch-size of 64, the multi-level multi-iterator would have 64 multi-iterators each with 4 iterators. Batches for the for the first thread would be constructed by taking the transactions pointed to by the first-iterator of each multi-iterator, the second thread would take the second-iterator of each multi-iterator, and so on. At each step, the first multi-iterator marches all its' positions forward, then the second multi-iterator marches its' positions forward, and so on.

An alternative approach, which I believe is equivalent, is to have 4 multi-iterators of 64 iterators each, and march the first position of each multi-iterator forward, then the second position of each multi-iterator forward, and so on; with batches being constructed by taking the transactions pointed to by the first multi-iterator, the second multi-iterator, and so on.

### Multi-Level Example

Let's assume we have 2 threads, and a batch-size of 4.

```text
Initialize:
[A, A, A, B, C, B, B, D, C, D, E, E, F, G, H, J, K]
 ^        ^
             ^        ^
                               ^     ^
                                        ^  ^

step:
[A, A, A, B, C, B, B, D, C, D, E, E, F, G, H, J, K]
    ^           ^
                         ^  ^
                                  ^           ^
                                                 ^

step:
[A, A, A, B, C, B, B, D, C, D, E, E, F, G, H, J, K]
       ^           ^
```

This gives batches:

```text
[
    ([A, C, E, G], [B, D, F, H]),
    ([A, C, E, K], [B, D, J]),
    ([A], [B]),
]
```

Note that the hottest events, `A` and `B`, are scheduled to separate threads. Note that this iteration does not guarantee that all other transactions do not conflict between threads. Logic will need to be implemented around holding locks on accounts for more than a single iteration, and releasing them when the transaction is executed, with the scheduler checking for freed locks during iteration. Additionally, queuing logic will benefit this approach such that `A` and `B` txs can be sent to the threads they are already scheduled on.
