use {
    crate::packet::Batch,
    rand::{thread_rng, Rng},
};

pub fn discard_batches_randomly<const N: usize>(
    batches: &mut Vec<Batch<N>>,
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

#[cfg(test)]
mod tests {
    use {super::*, crate::packet::Packet};

    #[test]
    fn test_batch_discard_random() {
        solana_logger::setup();
        let mut batch = Batch::<{ Packet::DATA_SIZE }>::default();
        batch.resize(1, Packet::default());
        let num_batches = 100;
        let mut batches = vec![batch; num_batches];
        let max = 5;
        discard_batches_randomly(&mut batches, max, num_batches);
        assert_eq!(batches.len(), max);
    }
}
