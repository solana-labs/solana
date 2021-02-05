use crate::rpc_config::RpcLargestAccountsConfig;
use crate::rpc_response::RpcAccountBalance;
use std::collections::HashMap;
use std::time::{Duration, SystemTime};

#[derive(Debug, Clone)]
pub struct LargestAccountsCache {
    size: usize,
    duration: u64,
    cache: HashMap<RpcLargestAccountsConfig, LargestAccountsCacheValue>,
}

#[derive(Debug, Clone)]
struct LargestAccountsCacheValue {
    accounts: Vec<RpcAccountBalance>,
    cached_time: SystemTime,
}

impl LargestAccountsCache {
    pub fn new(size: usize, duration: u64) -> Self {
        Self {
            size,
            duration,
            cache: HashMap::new(),
        }
    }

    pub fn get_largest_accounts(
        &self,
        config: &RpcLargestAccountsConfig,
    ) -> Option<Vec<RpcAccountBalance>> {
        self.cache.get(&config).map(|value| {
            value.cached_time.elapsed().ok().map(|elapsed| {
                if elapsed < Duration::from_secs(self.duration) {
                    Some(value.accounts.clone())
                } else {
                    None
                }
            })?
        })?
    }

    pub fn set_largest_accounts(
        &mut self,
        config: &RpcLargestAccountsConfig,
        accounts: &[RpcAccountBalance],
    ) {
        if self.cache.len() >= self.size {
            self.evict();
        }

        if self.cache.len() < self.size {
            self.cache.insert(
                config.clone(),
                LargestAccountsCacheValue {
                    accounts: accounts.to_owned(),
                    cached_time: SystemTime::now(),
                },
            );
        }
    }

    fn evict(&mut self) {
        let duration = Duration::from_secs(self.duration);
        self.cache.retain(|_key, value| {
            if let Ok(elapsed) = value.cached_time.elapsed() {
                elapsed < duration
            } else {
                false
            }
        });
    }
}

#[cfg(test)]
pub mod test {
    use crate::rpc_cache::LargestAccountsCache;
    use crate::rpc_config::{RpcLargestAccountsConfig, RpcLargestAccountsFilter};
    use crate::rpc_response::RpcAccountBalance;
    use std::time::Duration;

    #[test]
    fn test_cache_stays_within_size_limit() {
        let mut cache = LargestAccountsCache::new(2, 30);

        let config = RpcLargestAccountsConfig {
            commitment: None,
            filter: None,
        };

        let config2 = RpcLargestAccountsConfig {
            commitment: None,
            filter: Some(RpcLargestAccountsFilter::Circulating),
        };

        let config3 = RpcLargestAccountsConfig {
            commitment: None,
            filter: Some(RpcLargestAccountsFilter::NonCirculating),
        };

        let accounts: Vec<RpcAccountBalance> = Vec::new();

        cache.set_largest_accounts(&config, &accounts);
        cache.set_largest_accounts(&config2, &accounts);
        cache.set_largest_accounts(&config3, &accounts);

        assert_eq!(cache.cache.len(), 2);
    }

    #[test]
    fn test_old_entries_expire() {
        let mut cache = LargestAccountsCache::new(2, 1);

        let config = RpcLargestAccountsConfig {
            commitment: None,
            filter: None,
        };

        let accounts: Vec<RpcAccountBalance> = Vec::new();

        cache.set_largest_accounts(&config, &accounts);
        std::thread::sleep(Duration::from_secs(1));
        assert_eq!(cache.get_largest_accounts(&config), None);
    }

    #[test]
    fn test_old_entries_get_evicted() {
        let mut cache = LargestAccountsCache::new(2, 1);

        let config = RpcLargestAccountsConfig {
            commitment: None,
            filter: None,
        };

        let config2 = RpcLargestAccountsConfig {
            commitment: None,
            filter: Some(RpcLargestAccountsFilter::Circulating),
        };

        let config3 = RpcLargestAccountsConfig {
            commitment: None,
            filter: Some(RpcLargestAccountsFilter::NonCirculating),
        };

        let accounts: Vec<RpcAccountBalance> = Vec::new();

        cache.set_largest_accounts(&config, &accounts);
        cache.set_largest_accounts(&config2, &accounts);
        std::thread::sleep(Duration::from_secs(1));
        cache.set_largest_accounts(&config3, &accounts);
        assert_eq!(cache.cache.len(), 1);
    }

    #[test]
    fn test_entries_hashed_the_same() {
        let mut cache = LargestAccountsCache::new(2, 2);

        let config = RpcLargestAccountsConfig {
            commitment: None,
            filter: None,
        };

        let accounts: Vec<RpcAccountBalance> = vec![];

        cache.set_largest_accounts(&config, &accounts);
        cache.set_largest_accounts(&config, &accounts);
        assert_eq!(cache.cache.len(), 1);
    }
}
