use {
    crate::compute_unit_pricer::ComputeUnitPricer,
    lru::LruCache,
    solana_sdk::{clock::Slot, pubkey::Pubkey,},
};

/// Cache capacity is 2 times worst attacking blocks, which would have number of hot accounts
/// = 2 * (128 accounts/tx * 48M/6M txs) = 2048
const CACHE_CAPACITY: usize = 2048;

//TODO testing SIMD-0110, if it belongs to bank, it's abi
//#[frozen_abi(digest = "8upYCMG37Awf4FGQ5kKtZARHP1QfD2GMpQCPnwCCsxhu")]
//#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, AbiExample)]
#[derive(Debug)]
pub struct WriteLockFeeCache {
    cache: LruCache<Pubkey, ComputeUnitPricer>,
}

impl Default for WriteLockFeeCache {
    fn default() -> Self {
        Self::new(CACHE_CAPACITY)
    }
}

impl Clone for WriteLockFeeCache {
    fn clone(&self) -> Self {
        let mut other = LruCache::new(self.cache.cap());

        for (key, value) in self.cache.iter().rev() {
            other.push(*key, value.clone());
        }

        WriteLockFeeCache {
            cache: other,
        }
    }
}

impl WriteLockFeeCache {
    pub fn new(capacity: usize) -> Self {
        WriteLockFeeCache{
            cache: LruCache::new(capacity),
        }
    }

    // search cache for fee_rate for given pubkey.
    // return current fee_rate, in lamports_per_cu, if accout is in cache
    // return None otherwise
    pub fn get_write_lock_fee_rate(&self, pubkey: &Pubkey) -> Option<u64> {
        self.cache.peek(pubkey).map(|pricer| pricer.get_fee_rate())
    }

    // Update Cache with write locked accounts from just-frozen bank.
    // if account is in Cache, update its Pricer, adjusting fee rate higher or lower;
    // else, if account is "hot", evict cheapest account from Cache is at capacity, 
    // then add new hot account to Cache
    pub fn update(&mut self, slot: Slot, accounts: Vec<(Pubkey, u64)>) {
        accounts.iter().for_each(|(pubkey, cost)| {
            if self.has_account(pubkey) {
                self.update_account(slot, pubkey, cost);
            } else if self.is_hot_account(*cost) {
                if self.at_capacity() {
                    self.evict();
                }
                self.add_account(slot, pubkey, cost);
            }
        });
    }

    fn has_account(&self, pubkey: &Pubkey) -> bool {
        self.cache.contains(pubkey)
    }

    fn update_account(&mut self, slot: Slot, pubkey: &Pubkey, cost: &u64) {
        let pricer = self.cache.peek_mut(pubkey);
        if pricer.is_some() {
            pricer.unwrap().update(slot, *cost, crate::block_cost_limits::MAX_WRITABLE_ACCOUNT_UNITS);
        }
    }

    fn is_hot_account(&self, cost: u64) -> bool {
        cost > crate::block_cost_limits::MAX_WRITABLE_ACCOUNT_UNITS - cost
    }

    fn at_capacity(&self) -> bool {
        self.cache.len() == self.cache.cap()
    }

    fn add_account(&mut self, slot: Slot, pubkey: &Pubkey, cost: &u64) {
        let mut pricer = ComputeUnitPricer::default();
        pricer.update(slot, *cost, crate::block_cost_limits::MAX_WRITABLE_ACCOUNT_UNITS);
        self.cache.push(*pubkey, pricer);
    }

    // eviction policy:
    // At end of block, add new hot accounts to cache, if cache is full,
    // evict the account has lowest fee_rate. Designed to prevent cache attack
    fn evict(&mut self) {
        // TODO, if to impl own evidtin policy, then dont need to use LruCache.
    }
}
