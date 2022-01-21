use {
    crate::{
        data_budget::DataBudget,
        packet::{Packet, PacketBatch},
    },
    ahash::AHasher,
    rand::{thread_rng, Rng},
    std::{
        collections::HashMap,
        hash::{Hash, Hasher},
        net::IpAddr,
        sync::atomic::{AtomicUsize, Ordering},
    },
};

pub type StakeMap = HashMap<IpAddr, (u64, DataBudget)>;

pub struct Qos {
    ip_to_budget: Vec<DataBudget>,
    seed: (u128, u128),
    default_budget: DataBudget,
    max_packets_per_second: usize,
}

impl Qos {
    pub fn new(size: usize, max_packets_per_second: usize) -> Self {
        let mut ip_to_budget = Vec::with_capacity(size);
        ip_to_budget.resize_with(size, Default::default);
        let seed = (thread_rng().gen(), thread_rng().gen());
        let default_budget = DataBudget::default();
        Self {
            ip_to_budget,
            seed,
            default_budget,
            max_packets_per_second,
        }
    }

    // return true if newly discarded
    fn qos_packet(
        &self,
        total_stake: u64,
        staked_ip_to_budget: &StakeMap,
        packet: &mut Packet,
        now: u64,
        stats: &mut QosStats,
    ) -> bool {
        if packet.meta.discard() {
            return false;
        }

        let mut newly_discarded = false;
        if let Some((stake, budget)) = staked_ip_to_budget.get(&packet.meta.addr().ip()) {
            stats.num_staked = stats.num_staked.wrapping_add(1);
            budget.update_with_now(now, 1000, |_| {
                // u128 faster than f32 in benchmarking on cascade lake
                ((*stake as u128)
                    .saturating_mul(self.max_packets_per_second as u128)
                    .saturating_div(total_stake as u128)) as usize
            });
            if !budget.take(1) {
                stats.discard_staked = stats.discard_staked.wrapping_add(1);
                packet.meta.set_discard(true);
                return true;
            }
        } else {
            stats.num_unstaked = stats.num_unstaked.wrapping_add(1);
            let mut hasher = AHasher::new_with_keys(self.seed.0, self.seed.1);
            packet.meta.addr().ip().hash(&mut hasher);
            let hash = hasher.finish();
            let pos = (usize::try_from(hash).unwrap()).wrapping_rem(self.ip_to_budget.len());

            let ip_budget = &self.ip_to_budget[pos];
            ip_budget.update_with_now(now, 1000, |_| self.max_packets_per_second as usize / 100);
            if !ip_budget.take(1) {
                stats.discard_unstaked_per_ip = stats.discard_unstaked_per_ip.wrapping_add(1);
                packet.meta.set_discard(true);
                newly_discarded = true;
            }

            self.default_budget
                .update_with_now(now, 1000, |_| self.max_packets_per_second as usize / 10);
            if !self.default_budget.take(1) {
                stats.discard_unstaked_overall = stats.discard_unstaked_overall.wrapping_add(1);
                packet.meta.set_discard(true);
                return true;
            }
        }

        newly_discarded
    }
}

#[derive(Default, Debug)]
struct QosStats {
    num_staked: u64,
    num_unstaked: u64,
    discard_staked: u64,
    discard_unstaked_overall: u64,
    discard_unstaked_per_ip: u64,
}

pub fn qos_packets(
    qos_state: &Qos,
    total_stake: u64,
    staked_ip_to_budget: &StakeMap,
    batches: &mut [PacketBatch],
) -> usize {
    let num_discarded = AtomicUsize::new(0);
    let now = solana_sdk::timing::timestamp();
    let mut stats = QosStats::default();
    batches.iter_mut().for_each(|batch| {
        let mut discarded: usize = 0;
        batch.packets.iter_mut().for_each(|p| {
            if qos_state.qos_packet(total_stake, staked_ip_to_budget, p, now, &mut stats) {
                discarded = discarded.wrapping_add(1);
            }
        });
        num_discarded.fetch_add(discarded, Ordering::Relaxed);
    });
    num_discarded.load(Ordering::Relaxed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{packet::to_packet_batches, test_tx::test_tx};
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};

    #[test]
    fn test_qos_packet() {
        solana_logger::setup();
        let num_packets = 10;
        let mut batches = to_packet_batches(
            &(0..num_packets).map(|_| test_tx()).collect::<Vec<_>>(),
            128,
        );
        let mut staked_ip_to_budget = StakeMap::default();
        let max_packets_per_second = 100;
        let qos = Qos::new(100, max_packets_per_second);
        let total_stake = 100;
        // all unstaked, which should let 1 per second, so all but 1 will be rejected
        assert_eq!(
            num_packets - 1,
            qos_packets(&qos, total_stake, &staked_ip_to_budget, &mut batches)
        );

        let sender = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1));
        staked_ip_to_budget.insert(sender, (50, DataBudget::default()));

        batches.iter_mut().for_each(|batch| {
            batch.packets.iter_mut().for_each(|p| {
                let socket = SocketAddr::new(sender, 0);
                p.meta.set_addr(&socket);
                p.meta.set_discard(false);
            })
        });

        // with 50% stake, get's 50% of max_packets_per_second
        assert_eq!(
            0,
            qos_packets(&qos, total_stake, &staked_ip_to_budget, &mut batches)
        );
    }
}
