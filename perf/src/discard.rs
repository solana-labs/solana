use {
    crate::{packet::PacketBatch, tx_packet_batch::TxPacketBatch},
    rand::{thread_rng, Rng},
};

pub trait Batch {
    fn len(&self) -> usize;
}
impl Batch for PacketBatch {
    fn len(&self) -> usize {
        self.len()
    }
}
impl Batch for TxPacketBatch {
    fn len(&self) -> usize {
        self.len()
    }
}

pub fn discard_batches_randomly<T: Batch>(
    batches: &mut Vec<T>,
    max_packets: usize,
    mut total_packets: usize,
) -> usize {
    while total_packets > max_packets {
        let index = thread_rng().gen_range(0, batches.len());
        let removed = batches.swap_remove(index);
        total_packets = total_packets.saturating_sub(removed.len());
    }
    total_packets
}

// TODO: Moved here to reuse trait, but probably need a better
//       spot for this function (and maybe the trait too)
pub fn count_packets_in_batches<T: Batch>(batches: &[T]) -> usize {
    batches.iter().map(|batch| batch.len()).sum()
}

#[cfg(test)]
mod tests {
    use {super::*, crate::packet::Packet};

    #[test]
    fn test_batch_discard_random() {
        solana_logger::setup();
        let mut batch = PacketBatch::default();
        batch.resize(1, Packet::default());
        let num_batches = 100;
        let mut batches = vec![batch; num_batches];
        let max = 5;
        discard_batches_randomly(&mut batches, max, num_batches);
        assert_eq!(batches.len(), max);
    }
}
