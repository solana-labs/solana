use crate::accounts_index::IndexValue;
use crate::bucket_map_holder_stats::BucketMapHolderStats;
use crate::waitable_condvar::WaitableCondvar;
use std::fmt::Debug;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

// will eventually hold the bucket map
#[derive(Default)]
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
    pub fn new() -> Self {
        Self::default()
    }

    // intended to execute in a bg thread
    pub fn background(&self, exit: Arc<AtomicBool>, wait: Arc<WaitableCondvar>) {
        loop {
            wait.wait_timeout(Duration::from_millis(10000)); // account index stats every 10 s
            if exit.load(Ordering::Relaxed) {
                break;
            }
            self.stats.report_stats();
        }
    }
}
