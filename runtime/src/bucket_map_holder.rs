use crate::accounts_index::IndexValue;
use crate::bucket_map_holder_stats::BucketMapHolderStats;
use std::fmt::Debug;

// will eventually hold the bucket map
pub struct BucketMapHolder<T: IndexValue> {
    pub stats: BucketMapHolderStats,
    _phantom: std::marker::PhantomData<T>,
}

impl<T: IndexValue> Debug for BucketMapHolder<T> {
    fn fmt(&self, _f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        Ok(())
    }
}

impl<T: IndexValue> BucketMapHolder<T> {
    pub fn new(_bins: usize) -> Self {
        Self {
            stats: BucketMapHolderStats::default(),
            _phantom: std::marker::PhantomData::<T>::default(),
        }
    }
}
