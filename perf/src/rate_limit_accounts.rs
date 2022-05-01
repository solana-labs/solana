use {
    crate::{
        data_budget::DataBudget,
        packet::{Packet, PacketBatch},
        sigverify::get_packet_offsets,
    },
    ahash::AHasher,
    rand::{thread_rng, Rng},
    solana_sdk::{pubkey::Pubkey, timing::timestamp},
    std::{hash::Hasher, mem::size_of},
};

enum RateLimitEntry {
    Go(DataBudget),
    Stop(u64),
}

pub struct RateLimitByPubkey {
    limits: Vec<RateLimitEntry>,
    seed: (u128, u128),
    timeout_ms: u64,
}

pub const DEFAULT_TIMEOUT_MS: u64 = 10_000;
const RATE_LIMIT_TABLE_SIZE: usize = 10_007;

impl RateLimitByPubkey {
    pub fn new(timeout_ms: u64) -> Self {
        let limits: Vec<_> = (0..RATE_LIMIT_TABLE_SIZE)
            .into_iter()
            .map(|_| RateLimitEntry::Go(DataBudget::default()))
            .collect();
        Self {
            limits,
            seed: (thread_rng().gen(), thread_rng().gen()),
            timeout_ms,
        }
    }

    pub fn rate_limit_batch(&mut self, batches: &mut [PacketBatch]) {
        let now = timestamp();
        batches.iter_mut().for_each(|batch| {
            batch
                .packets
                .iter_mut()
                .for_each(|p| self.rate_limit_packet(now, p))
        });
    }

    fn rate_limit_packet(&mut self, now: u64, packet: &mut Packet) {
        if packet.meta.discard() {
            return;
        }

        let packet_offsets = get_packet_offsets(packet, 0, false);
        if packet_offsets.sig_len == 0 {
            packet.meta.set_discard(true);
            return;
        }

        for i in 0..(packet_offsets.pubkey_len as usize) {
            let mut hasher = AHasher::new_with_keys(self.seed.0, self.seed.1);
            let pubkey_start = i
                .saturating_mul(size_of::<Pubkey>())
                .saturating_add(packet_offsets.pubkey_start as usize);
            let pubkey_end = pubkey_start.saturating_add(size_of::<Pubkey>());
            hasher.write(&packet.data[pubkey_start..pubkey_end]);
            let hash = hasher.finish();
            let len = self.limits.len();
            let pos = (usize::try_from(hash).unwrap()).wrapping_rem(len);
            if let RateLimitEntry::Stop(stop_timestamp) = self.limits[pos] {
                if now > stop_timestamp {
                    self.limits[pos] = RateLimitEntry::Go(DataBudget::default());
                } else {
                    packet.meta.set_discard(true);
                    break;
                }
            }
            if let RateLimitEntry::Go(budget) = &self.limits[pos] {
                const RATE_LIMIT_UPDATE_MS: u64 = 100; // update every 100ms
                const RATE_LIMIT_UPDATE_PACKETS: usize = 100; // 100 packets per 100ms
                budget.update(RATE_LIMIT_UPDATE_MS, |old| {
                    old.saturating_add(RATE_LIMIT_UPDATE_PACKETS)
                });
                if !budget.take(1) {
                    self.limits[pos] = RateLimitEntry::Stop(now.saturating_add(self.timeout_ms));
                    packet.meta.set_discard(true);
                    break;
                }
            }
        }
    }

    pub fn num_blocked(&mut self) -> u64 {
        self.limits
            .iter()
            .map(|x| match x {
                RateLimitEntry::Stop(_) => 1,
                _ => 0,
            })
            .sum()
    }

    pub fn reset(&mut self) {
        for x in self.limits.iter_mut() {
            *x = RateLimitEntry::Go(DataBudget::default());
        }
        self.seed = (thread_rng().gen(), thread_rng().gen())
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::{packet::to_packet_batches, test_tx::test_tx},
        std::{thread::sleep, time::Duration},
    };

    fn check_discard(value: bool, batches: &[PacketBatch]) {
        for (_bi, batch) in batches.iter().enumerate() {
            for (_pi, packet) in batch.packets.iter().enumerate() {
                if packet.meta.discard() {
                    trace!("{} {}", _bi, _pi);
                }
                assert!(value == packet.meta.discard());
            }
        }
    }

    #[test]
    fn test_rate_limit() {
        solana_logger::setup();
        let mut limit = RateLimitByPubkey::new(100);
        let tx = test_tx();
        let mut batches =
            to_packet_batches(&std::iter::repeat(tx).take(100).collect::<Vec<_>>(), 10);

        limit.rate_limit_batch(&mut batches);
        check_discard(false, &batches);

        limit.rate_limit_batch(&mut batches);
        check_discard(true, &batches);
        for batch in batches.iter_mut() {
            for packet in batch.packets.iter_mut() {
                packet.meta.set_discard(false);
            }
        }

        sleep(Duration::from_millis(200));

        check_discard(false, &batches);
        info!("rate limit after sleep");
        limit.rate_limit_batch(&mut batches);
        check_discard(false, &batches);
    }
}
