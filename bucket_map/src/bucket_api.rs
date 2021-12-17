use {
    crate::{
        bucket::Bucket, bucket_item::BucketItem, bucket_map::BucketMapError,
        bucket_stats::BucketMapStats, MaxSearch, RefCount,
    },
    solana_sdk::pubkey::Pubkey,
    std::{
        ops::RangeBounds,
        path::PathBuf,
        sync::{
            atomic::{AtomicU64, Ordering},
            Arc, RwLock, RwLockWriteGuard,
        },
    },
};

type LockedBucket<T> = RwLock<Option<Bucket<T>>>;

pub struct BucketApi<T: Clone + Copy> {
    drives: Arc<Vec<PathBuf>>,
    max_search: MaxSearch,
    pub stats: Arc<BucketMapStats>,

    bucket: LockedBucket<T>,
    count: Arc<AtomicU64>,
}

impl<T: Clone + Copy> BucketApi<T> {
    pub fn new(
        drives: Arc<Vec<PathBuf>>,
        max_search: MaxSearch,
        stats: Arc<BucketMapStats>,
    ) -> Self {
        Self {
            drives,
            max_search,
            stats,
            bucket: RwLock::default(),
            count: Arc::default(),
        }
    }

    /// Get the items for bucket
    pub fn items_in_range<R>(&self, range: &Option<&R>) -> Vec<BucketItem<T>>
    where
        R: RangeBounds<Pubkey>,
    {
        self.bucket
            .read()
            .unwrap()
            .as_ref()
            .map(|bucket| bucket.items_in_range(range))
            .unwrap_or_default()
    }

    /// Get the Pubkeys
    pub fn keys(&self) -> Vec<Pubkey> {
        self.bucket
            .read()
            .unwrap()
            .as_ref()
            .map_or_else(Vec::default, |bucket| bucket.keys())
    }

    /// Get the values for Pubkey `key`
    pub fn read_value(&self, key: &Pubkey) -> Option<(Vec<T>, RefCount)> {
        self.bucket.read().unwrap().as_ref().and_then(|bucket| {
            bucket
                .read_value(key)
                .map(|(value, ref_count)| (value.to_vec(), ref_count))
        })
    }

    pub fn bucket_len(&self) -> u64 {
        self.count.load(Ordering::Relaxed)
    }

    pub fn delete_key(&self, key: &Pubkey) {
        let mut bucket = self.get_write_bucket();
        if let Some(bucket) = bucket.as_mut() {
            bucket.delete_key(key)
        }
    }

    fn get_write_bucket(&self) -> RwLockWriteGuard<Option<Bucket<T>>> {
        let mut bucket = self.bucket.write().unwrap();
        if bucket.is_none() {
            *bucket = Some(Bucket::new(
                Arc::clone(&self.drives),
                self.max_search,
                Arc::clone(&self.stats),
                Arc::clone(&self.count),
            ));
        } else {
            let write = bucket.as_mut().unwrap();
            write.handle_delayed_grows();
        }
        bucket
    }

    pub fn addref(&self, key: &Pubkey) -> Option<RefCount> {
        self.get_write_bucket()
            .as_mut()
            .and_then(|bucket| bucket.addref(key))
    }

    pub fn unref(&self, key: &Pubkey) -> Option<RefCount> {
        self.get_write_bucket()
            .as_mut()
            .and_then(|bucket| bucket.unref(key))
    }

    pub fn insert(&self, pubkey: &Pubkey, value: (&[T], RefCount)) {
        let mut bucket = self.get_write_bucket();
        bucket.as_mut().unwrap().insert(pubkey, value)
    }

    pub fn grow(&self, err: BucketMapError) {
        // grows are special - they get a read lock and modify 'reallocated'
        // the grown changes are applied the next time there is a write lock taken
        if let Some(bucket) = self.bucket.read().unwrap().as_ref() {
            bucket.grow(err)
        }
    }

    pub fn update<F>(&self, key: &Pubkey, updatefn: F)
    where
        F: FnMut(Option<(&[T], RefCount)>) -> Option<(Vec<T>, RefCount)>,
    {
        let mut bucket = self.get_write_bucket();
        bucket.as_mut().unwrap().update(key, updatefn)
    }

    pub fn try_write(
        &self,
        pubkey: &Pubkey,
        value: (&[T], RefCount),
    ) -> Result<(), BucketMapError> {
        let mut bucket = self.get_write_bucket();
        bucket.as_mut().unwrap().try_write(pubkey, value.0, value.1)
    }
}
