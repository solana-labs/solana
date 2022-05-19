//! The `bounded_streamer` module defines a set of services for safely & efficiently pushing & pulling packet batches between threads.
//!

use {
    crate::packet::PacketBatch,
    crossbeam_channel::{Receiver, RecvError, RecvTimeoutError, SendError, Sender, TrySendError},
    crossbeam_queue::ArrayQueue,
    solana_perf::packet::PACKETS_PER_BATCH,
    std::{
        cmp::min,
        result::Result,
        sync::{
            atomic::{AtomicUsize, Ordering},
            Arc,
        },
        time::{Duration, Instant},
    },
};

/// 10k batches means up to 1.3M packets, roughly 1.6GB of memory
pub const DEFAULT_MAX_QUEUED_BATCHES: usize = 10_000;

struct PacketBatchChannelData {
    queue: ArrayQueue<PacketBatch>,
    /// Number of packets currently queued
    packet_count: AtomicUsize,
    /// How many senders are sending to this channel
    sender_count: AtomicUsize,
}

#[derive(Clone)]
pub struct BoundedPacketBatchReceiver {
    signal_receiver: Receiver<()>,
    /// Instance of sender for waking up receivers (e.g. because there are more batches to receive)
    signal_sender: Sender<()>,
    data: Arc<PacketBatchChannelData>,
}

pub struct BoundedPacketBatchSender {
    signal_sender: Sender<()>,
    data: Arc<PacketBatchChannelData>,
}

impl Clone for BoundedPacketBatchSender {
    fn clone(&self) -> Self {
        {
            self.data.sender_count.fetch_add(1, Ordering::SeqCst);
        }
        Self {
            signal_sender: self.signal_sender.clone(),
            data: self.data.clone(),
        }
    }
}

impl Drop for BoundedPacketBatchSender {
    fn drop(&mut self) {
        let dropping_last_sender = self.data.sender_count.fetch_sub(1, Ordering::SeqCst) == 1;
        if dropping_last_sender {
            // notify receivers, otherwise they may be waiting forever on a
            // disconnected channel
            self.signal_sender.try_send(()).unwrap_or(());
        }
    }
}

impl PacketBatchChannelData {
    fn add_packet_count(&self, amount: usize) {
        self.packet_count.fetch_add(amount, Ordering::Relaxed);
    }
    fn sub_packet_count(&self, amount: usize) {
        self.packet_count.fetch_sub(amount, Ordering::Relaxed);
    }
}

enum TryRecvError {
    NoData,
    DisconnectedAndNoData,
}

impl BoundedPacketBatchReceiver {
    /// Receives up to around `packet_count_hint` packets from the channel.
    ///
    /// If no data is available, the function will wait up to `timeout`, then
    /// return RecvTimeoutError::Timeout if no data came in.
    ///
    /// If all senders have been dropped and there's no data avilable, it returns
    /// RecvTimeoutError::Disconnected.
    ///
    /// Returns (list of batches, packet count)
    pub fn recv_timeout(
        &self,
        packet_count_hint: usize,
        timeout: Duration,
    ) -> Result<(Vec<PacketBatch>, usize), RecvTimeoutError> {
        let deadline = Instant::now() + timeout;
        loop {
            self.signal_receiver.recv_deadline(deadline)?;
            return match self.try_recv(packet_count_hint) {
                Ok(r) => Ok(r),
                Err(TryRecvError::NoData) => continue,
                Err(TryRecvError::DisconnectedAndNoData) => Err(RecvTimeoutError::Disconnected),
            };
        }
    }

    /// Receives up to around `packet_count_hint` packets from the channel.
    ///
    /// Waits until there's data or the channel has been disconnected.
    ///
    /// Returns (vec-of-batches, packet count)
    pub fn recv(&self, packet_count_hint: usize) -> Result<(Vec<PacketBatch>, usize), RecvError> {
        loop {
            self.signal_receiver.recv()?;
            return match self.try_recv(packet_count_hint) {
                Ok(r) => Ok(r),
                Err(TryRecvError::NoData) => continue,
                Err(TryRecvError::DisconnectedAndNoData) => Err(RecvError),
            };
        }
    }

    // Returns (vec-of-batches, packet-count)
    fn try_recv(
        &self,
        packet_count_hint: usize,
    ) -> Result<(Vec<PacketBatch>, usize), TryRecvError> {
        let mut packets = 0;
        let disconnected = self.data.sender_count.load(Ordering::SeqCst) == 0;

        let mut batches = Vec::with_capacity(min(
            packet_count_hint / PACKETS_PER_BATCH,
            self.data.queue.len(),
        ));
        while let Some(batch) = self.data.queue.pop() {
            packets += batch.packets.len();
            batches.push(batch);
            if packets >= packet_count_hint {
                break;
            }
        }

        if batches.is_empty() {
            return if disconnected {
                // Wake up ourselves or other receivers again
                self.signal_sender.try_send(()).unwrap_or(());
                Err(TryRecvError::DisconnectedAndNoData)
            } else {
                Err(TryRecvError::NoData)
            };
        }

        self.data.sub_packet_count(packets);
        let has_more = !self.data.queue.is_empty();

        // If there's more data in the queue, then notify another receiver.
        // Also, if we're disconnected but still return data, wake up again to
        // signal the disconnected error without delay.
        if has_more || disconnected {
            self.signal_sender.try_send(()).unwrap_or(());
        }

        Ok((batches, packets))
    }

    /// Like `recv_timeout()` with a 1s timeout
    pub fn recv_default_timeout(
        &self,
        packet_count_hint: usize,
    ) -> Result<(Vec<PacketBatch>, usize), RecvTimeoutError> {
        self.recv_timeout(packet_count_hint, Duration::new(1, 0))
    }

    /// Like `recv_default_timeout()` that also measures its own runtime.
    pub fn recv_duration_default_timeout(
        &self,
        packet_count_hint: usize,
    ) -> Result<(Vec<PacketBatch>, usize, Duration), RecvTimeoutError> {
        let now = Instant::now();
        match self.recv_default_timeout(packet_count_hint) {
            Ok((batches, packets)) => Ok((batches, packets, now.elapsed())),
            Err(err) => Err(err),
        }
    }

    /// Number of batches in the queue
    pub fn batch_count(&self) -> usize {
        self.data.queue.len()
    }

    /// Number of packets in the queue
    pub fn packet_count(&self) -> usize {
        self.data.packet_count.load(Ordering::Relaxed)
    }
}

impl BoundedPacketBatchSender {
    /// Sends a single batch.
    ///
    /// If the queue was full and an old batch needed to be discarded, it returns
    /// Ok(true), otherwise Ok(false).
    ///
    /// SendErrors happen when all receivers have been dropped.
    pub fn send_batch(&self, batch: PacketBatch) -> Result<bool, SendError<()>> {
        if batch.packets.is_empty() {
            return Ok(false);
        }
        let mut discarded_batches = 0;
        self.push(batch, &mut discarded_batches);

        // wake a receiver
        match self.signal_sender.try_send(()) {
            Err(TrySendError::Disconnected(v)) => Err(SendError(v)),
            _ => Ok(discarded_batches > 0),
        }
    }

    /// Sends several batches.
    ///
    /// Returns the number of old batches that needed to be dropped to make space
    /// for the new.
    ///
    /// SendErrors happen when all receivers have been dropped.
    pub fn send_batches(
        &self,
        batches: Vec<PacketBatch>,
    ) -> std::result::Result<usize, SendError<()>> {
        if batches.is_empty() {
            return Ok(0);
        }

        let mut discarded_batches = 0;

        // discard early, to avoid lots of push fails in the common case
        let new_estimated_len = self.data.queue.len() + batches.len();
        if new_estimated_len > self.data.queue.capacity() {
            for _ in 0..(new_estimated_len - self.data.queue.capacity()) {
                self.maybe_discard_one(&mut discarded_batches);
            }
        }

        // insert all
        for batch in batches {
            self.push(batch, &mut discarded_batches);
        }

        // wake a receiver
        match self.signal_sender.try_send(()) {
            Err(TrySendError::Disconnected(v)) => Err(SendError(v)),
            _ => Ok(discarded_batches),
        }
    }

    /// Tries to discard a batch and increments discarded_batches on success.
    fn maybe_discard_one(&self, discarded_batches: &mut usize) {
        if let Some(b) = self.data.queue.pop() {
            self.data.sub_packet_count(b.packets.len());
            *discarded_batches += 1;
        }
    }

    /// Pushes a batch, potentially discarding others to make it happen.
    fn push(&self, mut batch: PacketBatch, discarded_batches: &mut usize) {
        self.data.add_packet_count(batch.packets.len());
        while let Err(uninserted_batch) = self.data.queue.push(batch) {
            batch = uninserted_batch;
            self.maybe_discard_one(discarded_batches);
        }
    }

    /// Number of batches in the queue
    pub fn batch_count(&self) -> usize {
        self.data.queue.len()
    }

    /// Number of packets in the queue
    pub fn packet_count(&self) -> usize {
        self.data.packet_count.load(Ordering::Relaxed)
    }
}

/// Creates the sender and receiver for a channel of packet batches.
///
/// The channel is multi-producer multi-consumer.
///
/// It is bounded to hold at most `max_queued_batches`. If more batches are
/// pushed into the channel, the oldest queued batches will be discarded.
///
/// It also counts the number of packets that are available in the channel,
/// but is not aware of a Packet's meta.discard flag.
pub fn packet_batch_channel(
    max_queued_batches: usize,
) -> (BoundedPacketBatchSender, BoundedPacketBatchReceiver) {
    let (signal_sender, signal_receiver) = crossbeam_channel::bounded::<()>(1);
    let data = Arc::new(PacketBatchChannelData {
        queue: ArrayQueue::new(max_queued_batches),
        packet_count: AtomicUsize::new(0),
        sender_count: AtomicUsize::new(1),
    });
    let sender = BoundedPacketBatchSender {
        signal_sender: signal_sender.clone(),
        data: data.clone(),
    };
    let receiver = BoundedPacketBatchReceiver {
        signal_receiver,
        signal_sender,
        data,
    };
    (sender, receiver)
}

#[cfg(test)]
mod test {
    use {
        super::*,
        solana_perf::packet::{Packet, PacketBatch},
    };

    #[test]
    fn bounded_streamer_test() {
        let num_packets = 10;
        let packets_batch_size = 50;
        let max_batches = 10;
        let (sender, receiver) = packet_batch_channel(max_batches);

        let mut packet_batch = PacketBatch::default();
        for _ in 0..num_packets {
            let p = Packet::default();
            packet_batch.packets.push(p);
        }

        // Case 1: Send a single batch
        match sender.send_batch(packet_batch.clone()) {
            Ok(dropped_packet) => assert!(!dropped_packet),
            Err(_err) => (),
        }

        match receiver.recv(packets_batch_size) {
            Ok((_batches, packets)) => assert_eq!(packets, num_packets),
            Err(_err) => (),
        }

        // Case2: Fully load the queue with batches
        let mut packet_batches = vec![];
        for _ in 0..max_batches + 1 {
            packet_batches.push(packet_batch.clone());
        }
        match sender.send_batches(packet_batches) {
            Ok(discarded) => assert_eq!(discarded, 1),
            Err(_err) => (),
        }

        // One batch should get dropped because queue is full.
        match sender.send_batch(packet_batch.clone()) {
            Ok(dropped_packet) => assert!(dropped_packet),
            Err(_err) => (),
        }

        // Receive batches up until the limit
        match receiver.recv(packets_batch_size) {
            Ok((batches, packets)) => {
                assert_eq!(batches.len(), packets_batch_size / num_packets);
                assert_eq!(packets, packets_batch_size);
            }
            Err(_err) => (),
        }

        // Receive the rest of the batches
        match receiver.recv(packets_batch_size) {
            Ok((batches, packets)) => {
                assert_eq!(batches.len(), packets_batch_size / num_packets);
                assert_eq!(packets, packets_batch_size);
            }
            Err(_err) => (),
        }
    }

    #[test]
    fn bounded_streamer_disconnect() {
        let num_packets = 10;
        let max_batches = 10;
        let timeout = Duration::from_millis(1);
        let (sender1, receiver) = packet_batch_channel(max_batches);

        let mut packet_batch = PacketBatch::default();
        for _ in 0..num_packets {
            let p = Packet::default();
            packet_batch.packets.push(p);
        }

        // CHECK: Receiving when no more data is present causes a timeout

        sender1.send_batch(packet_batch.clone()).unwrap();

        match receiver.recv(12) {
            Ok((batches, packets)) => {
                assert_eq!(batches.len(), 1);
                assert_eq!(packets, 10);
            }
            Err(_err) => (),
        }

        assert_eq!(
            receiver.recv_timeout(12, timeout).unwrap_err(),
            RecvTimeoutError::Timeout
        );

        // CHECK: Receiving when all senders are dropped causes Disconnected

        sender1.send_batch(packet_batch.clone()).unwrap();

        {
            let sender2 = sender1.clone();
            sender2.send_batch(packet_batch.clone()).unwrap();
        }

        drop(sender1);

        match receiver.recv(10) {
            Ok((batches, packets)) => {
                assert_eq!(batches.len(), 1);
                assert_eq!(packets, 10);
            }
            Err(_err) => (),
        }

        match receiver.recv(10) {
            Ok((batches, packets)) => {
                assert_eq!(batches.len(), 1);
                assert_eq!(packets, 10);
            }
            Err(_err) => (),
        }

        assert_eq!(
            receiver.recv_timeout(12, timeout).unwrap_err(),
            RecvTimeoutError::Disconnected
        );
    }
}
