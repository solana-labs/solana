use {
    crate::compute_unit_pricer::{MICRO_LAMPORTS_PER_LAMPORT, ComputeUnitPricer},
    lru::LruCache,
    solana_metrics::datapoint_info,
    solana_sdk::{clock::Slot, pubkey::Pubkey,},
};

/// Cache capacity is 2 times worst attacking blocks, which would have number of hot accounts
/// = 2 * (128 accounts/tx * 48M/6M txs) = 2048
const CACHE_CAPACITY: usize = 2048;

/// Initial value for write lock fee_rate per account, denominated in millilamports-per-cu
const DEFAULT_FEE_RATE: u64 = 0;

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

    // param: pubkeys are all writable locks of a tx.
    // If pubkey doesn't exist in Cache, then no write-lock-fee is charged for iti (default);
    // otherse the write-lock fee for that account is it's fee_rate times tx CU.
    // Return the total write locks fee, in lamports
    pub fn calculate_write_lock_fee(&self, pubkeys: &[Pubkey], cus: u32) -> u64 {
        let micro_lamports_fee = pubkeys.iter().map(|key| {
            let fee_rate_micro_lamports_per_cu = if let Some(fee_rate) = self.get_write_lock_fee_rate_micro_lamports_per_cu(key) {
                fee_rate
            } else {
                DEFAULT_FEE_RATE
            };
            u128::from(cus) * u128::from(fee_rate_micro_lamports_per_cu)
        })
        .sum::<u128>();

        micro_lamports_fee
            .saturating_add(MICRO_LAMPORTS_PER_LAMPORT.saturating_sub(1) as u128)
            .checked_div(MICRO_LAMPORTS_PER_LAMPORT as u128)
            .and_then(|fee| u64::try_from(fee).ok())
            .unwrap_or(u64::MAX)
    }

    // search cache for fee_rate for given pubkey.
    // return current fee_rate, in micro_lamports_per_cu if accout is in cache
    // return None otherwise
    pub fn get_write_lock_fee_rate_micro_lamports_per_cu(&self, pubkey: &Pubkey) -> Option<u64> {
        self.cache.peek(pubkey).map(|pricer| pricer.get_fee_rate_micro_lamports_per_cu())
    }

    // Update Cache with write locked accounts from just-frozen bank.
    // if account is in Cache, update its Pricer, adjusting fee rate higher or lower;
    // else, if account is "hot", evict cheapest account from Cache is at capacity, 
    // then add new hot account to Cache
    pub fn update(&mut self, slot: Slot, accounts: Vec<(Pubkey, u64)>) {
        // for accounts in cache but not in just-frozen bank's saturated accounts list
        // treat them as have `0` CU in current bank, so their ema can be updated/reduced quicker
        let (accounts, cus): (Vec<_>, Vec<_>) = accounts.into_iter().unzip();
        let mut cached_saturated_accounts: Vec<_> = self.cache.iter().filter_map(|(k, _)| {
            if accounts.contains(k) {
                None
            } else {
                Some((k.clone(), 0u64))
            }
        }).collect();
        
        let current_saturated_accounts = accounts.into_iter().zip(cus.into_iter());

        cached_saturated_accounts.extend(current_saturated_accounts);

        cached_saturated_accounts.iter().for_each(|(pubkey, cost)| {
            let (cache_changed, original_fee_rate) = if self.has_account(pubkey) {
                let original_fee_rate = self.cache.peek(pubkey).unwrap().get_fee_rate_micro_lamports_per_cu();
                self.update_account(slot, pubkey, cost);
                (true, original_fee_rate)
            } else if self.is_hot_account(*cost) {
                if self.at_capacity() {
                    self.evict();
                }
                self.add_account(slot, pubkey, cost);
                (true, 0)
            } else {
                (false, 0)
            };

            if cache_changed {
                let current_fee_rate = self.cache.peek(pubkey).unwrap().get_fee_rate_micro_lamports_per_cu();
                let current_ema = self.cache.peek(pubkey).unwrap().get_ema();
                // once a account's fee rate is dropped to `0`, it must be removed from cache,
                // otherwise future update will be = 0 * in[de]crease = 0;
                if current_fee_rate == 0 {
                    let _ = self.cache.pop(pubkey);
                }

                // report account per slot fee_rate
                datapoint_info!("simd-0110_account-stats",
                                ("slot", slot, i64),
                                ("pubkey", pubkey.to_string(), String),
                                ("parent_fee_rate", original_fee_rate, i64),
                                ("current_fee_rate", current_fee_rate, i64),
                                ("current_ema", current_ema, i64),
                                );
            }
        });

        // cache stats
        datapoint_info!("simd-0110-cache-stats", 
                        ("slot", slot, i64),
                        ("length", self.cache.len(), i64));
    }

    fn has_account(&self, pubkey: &Pubkey) -> bool {
        self.cache.contains(pubkey)
    }

    fn update_account(&mut self, slot: Slot, pubkey: &Pubkey, cost: &u64) {
        let pricer = self.cache.peek_mut(pubkey);
        if pricer.is_some() {
            pricer.unwrap().update(slot, *cost, crate::block_cost_limits::MAX_WRITABLE_ACCOUNT_UNITS)
                .report_metrics(pubkey);
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
        pricer.update(slot, *cost, crate::block_cost_limits::MAX_WRITABLE_ACCOUNT_UNITS)
            .report_metrics(pubkey);
        self.cache.push(*pubkey, pricer);
    }

    // eviction policy:
    // At end of block, add new hot accounts to cache, if cache is full,
    // evict the account has lowest fee_rate. Designed to prevent cache attack
    fn evict(&mut self) {
        // TODO, if to impl own evidtin policy, then dont need to use LruCache.
    }
}
