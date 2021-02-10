use crate::rpc_config::RpcLargestAccountsFilter;
use crate::rpc_response::RpcAccountBalance;
use std::collections::HashMap;
use std::time::{Duration, SystemTime};

#[derive(Debug, Clone)]
pub struct LargestAccountsCache {
    duration: u64,
    cache: HashMap<Option<RpcLargestAccountsFilter>, LargestAccountsCacheValue>,
}

#[derive(Debug, Clone)]
struct LargestAccountsCacheValue {
    accounts: Vec<RpcAccountBalance>,
    cached_time: SystemTime,
}

impl LargestAccountsCache {
    pub fn new(duration: u64) -> Self {
        Self {
            duration,
            cache: HashMap::new(),
        }
    }

    pub fn get_largest_accounts(
        &self,
        filter: &Option<RpcLargestAccountsFilter>,
    ) -> Option<Vec<RpcAccountBalance>> {
        self.cache.get(&filter).and_then(|value| {
            if let Ok(elapsed) = value.cached_time.elapsed() {
                if elapsed < Duration::from_secs(self.duration) {
                    return Some(value.accounts.clone());
                }
            }
            None
        })
    }

    pub fn set_largest_accounts(
        &mut self,
        filter: &Option<RpcLargestAccountsFilter>,
        accounts: &[RpcAccountBalance],
    ) {
        self.cache.insert(
            filter.clone(),
            LargestAccountsCacheValue {
                accounts: accounts.to_owned(),
                cached_time: SystemTime::now(),
            },
        );
    }
}

#[cfg(test)]
pub mod test {
    use crate::rpc_cache::LargestAccountsCache;
    use crate::rpc_config::RpcLargestAccountsFilter;
    use crate::rpc_response::RpcAccountBalance;
    use std::time::Duration;

    #[test]
    fn test_old_entries_expire() {
        let mut cache = LargestAccountsCache::new(1);

        let filter = Some(RpcLargestAccountsFilter::Circulating);

        let accounts: Vec<RpcAccountBalance> = Vec::new();

        cache.set_largest_accounts(&filter, &accounts);
        std::thread::sleep(Duration::from_secs(1));
        assert_eq!(cache.get_largest_accounts(&filter), None);
    }
}
