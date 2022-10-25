Propose to Refactor banking stage buffer

# Motivation

One of the limitations of current MinMaxHeap based banking-stage buffer is that it does not
include information of account contention, so it is possible that prepared batches are highly
contended, in extream case, all 64 packets in a batch write to same account.

This potential inefficiency was partly due to deserialized packets are not sanitized into
transactions until later time, therefore at the time of buffering, banking stage does not have
transaction's writable accounts information.

Since moving packets' deserialization and sanitization closer to the point of receiving becomes
necessary, it is now possible to include write accounts info in priority sorted buffer, so
banking threads can prepare batches of non-conflicting transactions for bank processing.

# Proposal

Buffer can be a structure of: `BTreeMap<Priority, Vec<AccountBuckets>>`, where AccountBuckets is
a collection of FIFO queues of packets, each FIFO queue contains packets write to same account.

For example: tx1 and tx3 have priority `99`, tx1 writes to A and B, tx3 writes C, would look
like this: 
[
  (99, [ ([tx1, ...]), // txs write to account A
         ([tx1, ...]), // txs write to account B
         ([tx3, ...]), // txs write to account C
         ... ...
       ]
  ),
  (98, [ ([tx1, ...]), // txs write to account A
         ([tx2, ...]), // txs write to account B
         ([tx3, ...]), // txs write to account D
         ... ...
       ]
  ),
  (...)
]

When consume this buffer, banking thread prepare a batch of non-conflicting transactions from
the top of priority, pop the oldest packet from non-conflicting account's bucket. In above
example, a batch could look like `[tx1, tx3, tx2, ...]`

# concerns

1. Too many priority level may hurt performance, 
2. Multi Iterator applied on top of current MinMaxHeap buffer archives same result; There are
   few benefits of refactoring buffer from clean code perspective, and perhaps some performance
   improvement. 

# bench tests
We have a set of bench tests for `unprocessed_packets_buffer`, as well as `baning-bench`, both
can be used to perform side-by-side comparison.

# pseudo code
```
use std::collections::BTreeMap;

/// Packets
struct DeserializedPacket {
    original_packet: Packet,
    sanitized_transaction: Option<Arc<SanitizedTransaction>>,
    priority: Priority,
    writable_accounts: Vec<&Pubkey>,
    attempted: bool,   // default false, mark to true when selected into batch for processing
    retryable: bool,   // default false, mark to true when packet is determined as retryable
    forwarded: bool,
    // ... ...
}

/// a FIFO queue for packets that write to same account at same priority
struct Packets {
    packets: deque<Arc<DeserializedPacket>>,
}

/// a vector of Packets for writable account hash
struct AccountBucket {
    // hashes account key into usize with local random seed
    account_hasher: AccountHasher,
    // account key is hashed into a slot in `accounts`, hash collision
    // can be tolerated if accounts capacity is adequate. 
    // On average, there are 5k accounts in each block.
    accounts: Vec<Packets>,
}

struct PrioritySortedAccountBucket {
    buffer: BtreeMap<u64, AccountBucket>,
}

impl AccountBucket {
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            account_hasher: AccountHasher::default(),
            accounts: vec![Packets::default(); capacity],
        }
    }

    /// create a new account_bucket, add `packet` as its first entry
    pub fn new(packet: Arc<DeserializedPacket>, writable_acocunts: &[&Pubkey]) -> Self {
        let mut new = Self::new(default_capacity);
        new.push(packet, writable_accounts);
        new
    }

    /// push a `packet` into back of account_bucket
    pub fn push(&mut self, packet: Arc<DeserializedPacket>, writable_acocunts: &[&Pubkey]) 
    {
        // a packet can be in multiple account buckets.
        for writable_acocunt in writable_accounts {
            let index = self.account_hasher.hash(writable_account);
            self.accounts[index].push_back(packet.clone());
        }
    }

    /// to take up to `num_of_packets_to_select` non-conflicting, process-able packets to form a
    /// batch
    pub fn take(
        &self,
        packet_batch: &mut Vec<Arc<DeserializedPacket>>,
        num_of_packets_to_select: usize
    ) {
        let mut packet_batch_count: usize = 0;

        // account keys were hashed with random seeds, should be OK to iterate from head of vec
        for packets in self.accounts {
            // use `peek` instead of `pop`, so to remove packets from buffer after they are
            // processed; another way is to `pop` here, but re-insert those retryable after
            // processing, which can be slower. 
            while let Some(packet) = packets.front() {

                // only select:
                // 1. (!attempted || retryable) packets, this also covers the
                // case that tx1 writes to both account A and B. After it is selected from A, it
                // will be skipped from B since it is `attempted == true`
                // 2. no conflict with selected packets in batch;
                if (!packet.attempted || packet.retryable) && 
                    no_conflicting_account(packet_batch, packet.writable_accounts) {
                    packet_batch.push(packet.clone());
                    packet.attempted = true;
                    packet_batch_count += 1;
                    break; // selected oldest packet from this account bucket, move to next account bucket
                }
            }
            if packet_batch_count >= num_of_packets_to_select {
                // selected requested number of packets, exit
                break;
            }
        }
    }
}


impl PrioritySortedAccountGroupedBuffer {
    /// take up to `count` non-conflicting packets from buffer in priority order, 
    /// returns packets picked
    pub fn take(&self, count: usize) -> Vec<Arc<DeserializedPacket>> {
        let mut packet_batch = Vec::with_capacity(count);
        let mut num_of_packets_to_take = count;

        // iterate buffer in priority order until a batch of non-conflicting packets are selected,
        // or end of buffer.
        for (prioty, account_buckets) in self.buffer.iter() {
            account_buckets.take(&mut packet_batch, num_of_packets_to_select);
            num_of_packets_to_select = count - packet_batch.len();
            if num_of_packets_to_select < 1 {
                break;
            }
        }
    }

    pub fn remove(&mut self, packet: Arc<DeserializedPacket>) {
        let mut accunt_grouped_buffer = self.buffer.get_mut(packet.priority);
        // remove it from all accounts buckets
        for writable_account in packet.writable_accounts {
            let index = self.account_hhasher.hash(writable_account);
            // pop all (attempted && !retryable) from account bucket
            let packets = self.accounts[index];
            while let Some(packet) = packets.peek() {
                if packet.attempted && !packet.retryable {
                    packets.pop();
                    continue;
                }
                // cut short iterating at first process-able packet
            }
        }
    }
}

fn receive_and_buffer_packets(buffer: &mut PrioritySortedAccountGroupedBuffer) {
    let packet = receiver.recv();

    // packet failed during deserializing and sanitizing are dropped before into to buffer
    // NOTE: can add additional filters here, such as drop invalid fee payer
    let sanitized_transaction = packet.get_or_build_sanitized_transaction()?;
    let priority = packet.get_priority();
    let writable_accounts = packet.get_writable_accounts();

    // insert to priority sorted buffer
    buffer
        .entry(priority)
        .and_modify(|account_grouped_buffer| account_grouped_buffer.push(packet, writable_accounts))
        .or_insert(AccountGroupedBuffer::new(packet, writable_accounts));
}

fn consume_buffer_packets(buffer: &mut PrioritySortedAccountGroupedBuffer) {
    const BATCH_SIZE: usize = 8;
    const NUMBER_OF_BATCHES_BEFORE_CHECK_POH: usize = 16;

    let mut slot_expired: bool = false;

    while !slot_expired {
        let mut batch_count: usize = 0;
        // BATCH_SIZE and NUMBER_OF_BATCHES_BEFORE_CHECK_POH are the knobs to tune to find balance
        // of not to let too many low priority packets starve high priority ones, and not to
        // increase bank's lock contention.
        while batch_count < NUMBER_OF_BATCHES_BEFORE_CHECK_POH {
            // collect a batch of non-conflicting packets in priority order for attempted
            // processing; 
            // only packets that are ((!attempted || retryable) && no-conflict) will be selected;
            // packets are selected in batch will be marked as `attempted = true`;
            let packets = buffer.take(BATCH_SIZE);

            // send batch of non-conflicting packets for bank processing. Returns processed_packets
            // that include those successfully executed or failed execution; those packets will be
            // removed from buffer
            let (slot_expired, processed_packets) = process_packets(packets);

            // remove non-retryable packets from buffer, eg packets are successfully executed and
            // committed, or failed on execution, are removed from buffer
            for packet in processed_packets {
                buffer.remove(packet);
            }

            if slot_expired {
                // simply stop consuming if slot ended 
                return;
            }
            batch_count += 1;
        }
        slot_expired = check_poh();
    }
}

fn process_loop() {
    let mut buffer: PrioritySortedAccountGroupedBuffer::default();

    loop {
        receive_and_buffer_packets(&mut buffer);

        consume_buffered_packet(&mut buffer);
    }
}
```
