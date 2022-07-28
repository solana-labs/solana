use solana_sdk::{clock::Slot, transaction::SanitizedTransaction};

pub trait PrioritizationFeeCacheUpdate {
    /// Update block's min prioritization fee with `txs`,
    /// Returns updated min prioritization fee for `slot`
    fn update_transactions<'a>(
        &mut self,
        slot: Slot,
        txs: impl Iterator<Item = &'a SanitizedTransaction>,
    ) -> Option<u64>;

    /// bank is completely replayed from blockstore; its fee stats can be made available to queries
    fn finalize_block(&mut self, slot: Slot);
}
