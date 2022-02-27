---
title: Leader QoS
---

## Motivation:
Banking thread has too many packets in its buffer, it needs an ability to pack
high-priority transactions into limited block space first.

## Scope:
Leader has multiple banking threads, each thread does:
1. Receives packet_batches from verified_receiver;
2. Push packet_batches into FIFO buffer;
3. Traverse FIFO buffer linearly to select packets for processing (aka
   consuming) or forwarding; Where:
   
   "consuming" is a process of preparing selected packets into batches of 
   sanitized_transactions, submitting for attempted bank execution, which in 
   turn includes block cost tracking, account locking, and executing/committing.
   Retryable transactions will be put back to buffer for next iteration.

   "forwarding" is a process of preparing selected unprocessed packets from
   unforwarded packet_batches, batch-send to Fwd ports. 

The aim of this proposal is to enhance the packet selection process for
"consuming" and "forwarding" within each banking_stage threads; by 
compartmentalizing hcanges to speed up delivery. A more holistic change 
proposal (#23438) exists. 

It propose to replace linear FIFO buffer traversing with transaction-based 
prioritization. The change scope is mainly at step #3, instead of linearly
selecting unprocessed packets from buffered packet_batches, it sorts 
unprocessed packets with following criteria to prioritize consuming and
forwarding. 

The "consuming" and "forwarding" processing as described above largely remains
the same, as they are outside scope of this proposal. 

## Prioritization criteria:
1. Transaction's fee-per-CU (`= (additional_fee + base_fee)/requested_CU)`), 
   higher paid transactions have priority to be first considered for block 
   inclusion or forwarding. 
   Note: `additional_fee` and `requested_cu` are from `compute_budget` i
   instruction.

2. When fee-per-CU is same, Transactions from higher staked validators have
   higher chance to be considered for blocking inclusion or forwarding.

## Implementation 
Because received packets are buffered as packet_batches, in order to 
prioritize transaction-by-transaction without additional copying of packets,
there are few extra required changes:
1. Deserialize packets into transactions upfront, before buffer traversing;
2. Introduce a way (eg., locator) of iterating packets from buffered 
   packet_batches;

### Proposed workflow:
Draft PR #23257 demonstrating the implementation of this proposal. High-level
changes are outlined below, `*` denotes a new/changed step, with its commit #.

#### packet receiving and buffering:
1. Receives packet_batches from verified_receiver

   How multiple banking_stage threads pulls packet batches from single source
   is not changed. There are some built-in randomness to distribute transactions
   across threads, ideally avoid scenario of one thread has all high-paid
   transactions and other threads all have low-paid transactions. However 
   unlikely it is, to guarantee avoid such unequal distribution, we'd need a 
   central scheduler as #23438 proposes.
 
2. * Deserialize packets into versioned_transactions upon receiving,
     cache to buffer with packets. Also refactored `buffer` into its own model.
     [commit #e74c5a](https://github.com/solana-labs/solana/pull/23257/commits/e74c5a792284629b242504b12ac5765fac0c773b)

3. * When need to drop packet_batch from buffer to make room for newly received,
     instead of popping out the oldest packet_batch which may include higher 
     prioritized packets, it drops the lowest prioritized packet (by marking it
     as processed) until an empty batch is found, then swap it with newly 
     received packet_batch.
     [commit #88d710](https://github.com/solana-labs/solana/pull/23257/commits/88d71067ef13b02ce1f03b329a7b013ee4f063d8)
 
#### packet consuming:
1. * Scan buffer to index unprocessed packets into `Vec<locator>`, also gather 
     each packet's sender stakes.
     [commit #d58a81a](https://github.com/solana-labs/solana/pull/23257/commits/d58a81a748ea9b10ee5b7aeca225fad3aad18e72)

     Buffer changes between each iteration, it must be re-prioritized before
     each consumption. This adds the `sorint` overhead, the additional benifit
     is allowing higher-paid transactions to "jump the queue", allow to be 
     considered for processing as soon as they are received.

2. * Add transaction weighting stage to add sender stake in packet meta fields.
     [commit #5900f84](https://github.com/solana-labs/solana/pull/23257/commits/5900f84ac06496625f77efd2a6152b6942ef50af)

3. * Shuffle locators by stake weight;
     [commit #6092b09](https://github.com/solana-labs/solana/pull/23257/commits/6092b095177f1a29f996aad13f41e7de4674f9eb)

4. * Then apply transaction fee-per-cu prioritization on top, such the locators
     are sorted by fee-per-cu then stake weights. 
     [commit #f8a6c41](https://github.com/solana-labs/solana/pull/23257/commits/f8a6c416eef7429f6af1d05eef241f6da5146a2e)
     [commit #a277cd8](https://github.com/solana-labs/solana/pull/23257/commits/a277cd871ab730d72b2e0cc420bfd5322d0ef939)

     Transaction's base fee is determined by fee-tructure, which could change
     between slot boundary. Therefore transaction's  fee-per-cu is calculated 
     with current working-bank, or the last working-bank.

5. * Batches packets from prioritized locators into chunks of sanitized 
     transactions.
     [commit #f30a6d7](https://github.com/solana-labs/solana/pull/23257/commits/f30a6d7c92c48a320aefb6493cbcd955d36ff9e8)

6. send chunk sanitized_transactions to consuming process (largely unchanged)

7. * Update buffer with retryable transactions.

#### forwarding:
1. * similar to above, scan and sort unprocessed packets of unforwarded batches
     into prioritized locators, prepare few buckets, fill the buckets with 
     sorted packets, forward buckets of packets.
     [commit #b5998cd](https://github.com/solana-labs/solana/pull/23257/commits/b5998cd161ac3af58be6ba0d3ea7803074cf69ff)


### Future works
Due to the banking_stage threads are independent from each other, the possibility
of one high-paid transaction received by one thread being starved by lower-paid
transactions in other threads exists. To address this before #23438, it is 
possible to introduce a global account state machine to keep track each write 
account state (available, locked, reserved).

