use solana_perf::packet::{PacketBatch};
use solana_perf::sigverify::{get_packet_offsets};
use crate::expiring_filter::ExpiringFilter;
use crate::fee_filter::FeeFilter;

pub struct PacketFilter {
    blockhash_filter: ExpiringFilter,
    fee_payer_filter: ExpiringFilter,
    fee_filter: FeeFilter,
}

impl PacketFilter {
    // allows for both false positives and false negatives
    pub fn filter_packets(&self, batches: &mut [PacketBatch], now_ms: u64) {
        for batch in batches {
            for packet in batch.packets.iter_mut() {
                if packet.meta.discard() {
                    continue;
                }
                let offsets = get_packet_offsets(packet, 0, false);
                if let Some(blockhash) = offsets.blockhash(packet) {
                    if !self
                        .blockhash_filter
                        .check(blockhash, now_ms)
                    {
                        packet.meta.set_discard(true);
                        continue;
                    }
                }
                if let Some(fee_payer) = offsets.fee_payer(packet) {
                    if !self
                        .fee_payer_filter
                        .check(fee_payer, now_ms)
                    {
                        packet.meta.set_discard(true);
                        continue;
                    }
                }
                if let Some(lamports_per_cu) = offsets.lamports_per_cu(packet) {
                    let mut discard = false;
                    for key in offsets.writable_accounts(packet) {
                        if !self
                            .fee_filter
                            .check_price(key, lamports_per_cu, now_ms)
                        {
                            discard = true;
                            break
                        }
                    }
                    if discard {
                        packet.meta.set_discard(true);
                    }
                }
            }
        }
    }
}
