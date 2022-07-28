use solana_sdk::pubkey::Pubkey;

pub trait PrioritizationFeeCacheQuery {
    /// Returns number of blocks that have finalized min fees collection
    fn available_block_count(&self) -> usize;

    /// Query block minimum fees from finalized blocks in cache,
    /// Returns a vector of fee; call site can use it to produce
    /// average, or top 5% etc.
    fn get_prioritization_fees(&self) -> Vec<u64>;

    /// Query given account minimum fees from finalized blocks in cache,
    /// Returns a vector of fee; call site can use it to produce
    /// average, or top 5% etc.
    fn get_account_prioritization_fees(&self, accoucnt_key: &Pubkey) -> Vec<u64>;
}
