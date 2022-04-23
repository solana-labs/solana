use solana_perf::packet::PacketBatch;
use crate::expiring_filter::ExpiringFilter;
use crate::fee_filter::FeeFilter;

struct PacketFilter {
    blockhash_filter: ExpiringFilter,
    fee_payer_filter: ExpiringFilter,
    fee_filter: FeeFilter,
}

impl PacketFilter {
    // allows for both false positives and false negatives
    pub fn filter_packets(&self, batches: &mut [PacketBatch], now_ms: u64) {
        for batch in batches {
            for packet in &mut batch.packets {
                if packet.meta.discard() {
                    continue;
                }
                let offsets = get_packet_offsets(p);
                if let Ok(blockhash) = offsets.get_packet_blockhash(p) {
                    if !self
                        .blockhash_filter
                        .check(blockhash, now_ms)
                    {
                        p.meta.set_discard(true);
                        continue;
                    }
                }
                if let Ok(fee_payer) = offsets.get_packet_fee_payer(p) {
                    if !self
                        .fee_payer_filter
                        .check(fee_payer, now_ms)
                    {
                        p.meta.set_discard(true);
                        continue;
                    }
                }
                if let Ok(lamports_per_cu) = offsets.get_lamports_per_cu(p) {
                    for keys in offsets.get_writable_accounts(p) {
                        if !self
                            .fee_filter
                            .check_price(lamports_per_cu, now_ms)
                        {
                            p.meta.set_discard(true);
                            continue;
                        }
                    }
                }
            }
        }
    }
}
