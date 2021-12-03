use {crate::RefCount, solana_sdk::pubkey::Pubkey};

#[derive(Debug, Default, Clone)]
pub struct BucketItem<T> {
    pub pubkey: Pubkey,
    pub ref_count: RefCount,
    pub slot_list: Vec<T>,
}
