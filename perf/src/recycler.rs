use log::*;
use rand::{thread_rng, Rng};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::Instant;
use std::{collections::VecDeque, sync::atomic::AtomicBool};

#[derive(Debug, Default)]
struct RecyclerStats {
    total: AtomicUsize,
    freed: AtomicUsize,
    reuse: AtomicUsize,
    max_gc: AtomicUsize,
}

#[derive(Clone, Default)]
pub struct Recycler<T> {
    recycler: Arc<RecyclerX<T>>,
}

#[derive(Debug)]
pub struct RecyclerX<T> {
    gc: Mutex<VecDeque<(Instant, T)>>,
    stats: RecyclerStats,
    id: usize,
}

impl<T: Default> Default for RecyclerX<T> {
    fn default() -> RecyclerX<T> {
        let id = thread_rng().gen_range(0, 1000);
        trace!("new recycler..{}", id);
        RecyclerX {
            gc: Mutex::new(VecDeque::new()),
            stats: RecyclerStats::default(),
            id,
        }
    }
}

pub trait Reset {
    fn reset(&mut self);
    fn warm(&mut self, size_hint: usize);
    fn set_recycler(&mut self, recycler: Weak<RecyclerX<Self>>)
    where
        Self: std::marker::Sized;
    fn unset_recycler(&mut self)
    where
        Self: std::marker::Sized;
}

lazy_static! {
    static ref WARM_RECYCLERS: AtomicBool = AtomicBool::new(false);
}

pub fn enable_recycler_warming() {
    WARM_RECYCLERS.store(true, Ordering::Relaxed);
}

fn warm_recyclers() -> bool {
    WARM_RECYCLERS.load(Ordering::Relaxed)
}

pub const MAX_INVENTORY_COUNT_WITHOUT_EXPIRATION: usize = 10;
pub const EXPIRATION_TTL_SECONDS: u64 = 3600;

impl<T: Default + Reset + Sized> Recycler<T> {
    pub fn warmed(num: usize, size_hint: usize) -> Self {
        let new = Self::default();
        if warm_recyclers() {
            let warmed_items: Vec<_> = (0..num)
                .map(|_| {
                    let mut item = new.allocate("warming");
                    item.warm(size_hint);
                    item
                })
                .collect();
            warmed_items
                .into_iter()
                .for_each(|i| new.recycler.recycle(i));
        }
        new
    }

    pub fn allocate(&self, name: &'static str) -> T {
        let new = {
            let mut gc = self.recycler.gc.lock().unwrap();

            // Don't expire when inventory is rather small. At least,
            // we shouldn't expire (= drop) the last inventory; we can just
            // return it from here.  Also add some buffer. Thus, we don't
            // expire if less than 10.
            if gc.len() > MAX_INVENTORY_COUNT_WITHOUT_EXPIRATION {
                if let Some((oldest_time, _old_item)) = gc.front() {
                    if oldest_time.elapsed().as_secs() >= EXPIRATION_TTL_SECONDS {
                        let (_, mut expired) = gc.pop_front().unwrap();
                        // unref-ing recycler here is crucial to prevent
                        // the expired from being recycled again via Drop,
                        // lading to dead lock!
                        expired.unset_recycler();
                    }
                }
            }
            gc.pop_back().map(|(_added_time, item)| item)
        };

        if let Some(mut x) = new {
            self.recycler.stats.reuse.fetch_add(1, Ordering::Relaxed);
            x.reset();
            return x;
        }

        let total = self.recycler.stats.total.fetch_add(1, Ordering::Relaxed);
        trace!(
            "allocating new: total {} {:?} id: {} reuse: {} max_gc: {}",
            total,
            name,
            self.recycler.id,
            self.recycler.stats.reuse.load(Ordering::Relaxed),
            self.recycler.stats.max_gc.load(Ordering::Relaxed),
        );

        let mut t = T::default();
        t.set_recycler(Arc::downgrade(&self.recycler));
        t
    }

    pub fn recycle_for_test(&self, x: T) {
        self.recycler.recycle(x);
    }
}

impl<T: Default + Reset> RecyclerX<T> {
    pub fn recycle(&self, x: T) {
        let len = {
            let mut gc = self.gc.lock().expect("recycler lock in pub fn recycle");
            gc.push_back((Instant::now(), x));
            gc.len()
        };

        let max_gc = self.stats.max_gc.load(Ordering::Relaxed);
        if len > max_gc {
            // this is not completely accurate, but for most cases should be fine.
            let _ = self.stats.max_gc.compare_exchange(
                max_gc,
                len,
                Ordering::Relaxed,
                Ordering::Relaxed,
            );
        }
        let total = self.stats.total.load(Ordering::Relaxed);
        let reuse = self.stats.reuse.load(Ordering::Relaxed);
        let freed = self.stats.total.fetch_add(1, Ordering::Relaxed);
        datapoint_debug!(
            "recycler",
            ("gc_len", len as i64, i64),
            ("total", total as i64, i64),
            ("freed", freed as i64, i64),
            ("reuse", reuse as i64, i64),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    const RESET_VALUE: u64 = 10;
    impl Reset for u64 {
        fn reset(&mut self) {
            *self += RESET_VALUE;
        }
        fn warm(&mut self, _size_hint: usize) {}
        fn set_recycler(&mut self, _recycler: Weak<RecyclerX<Self>>) {}
        fn unset_recycler(&mut self) {}
    }

    #[test]
    fn test_recycler() {
        let recycler = Recycler::default();
        let mut y: u64 = recycler.allocate("test_recycler1");
        assert_eq!(y, 0);
        y = 20;
        let recycler2 = recycler.clone();
        recycler2.recycle_for_test(y);
        assert_eq!(recycler.recycler.gc.lock().unwrap().len(), 1);
        let z = recycler.allocate("test_recycler2");
        assert_eq!(z, 20 + RESET_VALUE);
        assert_eq!(recycler.recycler.gc.lock().unwrap().len(), 0);
    }

    #[test]
    fn test_recycler_ttl() {
        let recycler = Recycler::default();
        recycler.recycle_for_test(42);
        recycler.recycle_for_test(43);
        assert_eq!(recycler.recycler.gc.lock().unwrap().len(), 2);

        // meddle to make the first element to expire
        recycler.recycler.gc.lock().unwrap().front_mut().unwrap().0 =
            Instant::now() - Duration::from_secs(EXPIRATION_TTL_SECONDS + 1);

        let y: u64 = recycler.allocate("test_recycler1");
        assert_eq!(y, 43 + RESET_VALUE);
        assert_eq!(recycler.recycler.gc.lock().unwrap().len(), 1);

        // create enough inventory to trigger expiration
        for i in 44..44 + MAX_INVENTORY_COUNT_WITHOUT_EXPIRATION as u64 {
            recycler.recycle_for_test(i);
        }
        assert_eq!(recycler.recycler.gc.lock().unwrap().len(), 11);

        // allocate with expiration causes len to be reduced by 2
        let y: u64 = recycler.allocate("test_recycler1");
        assert_eq!(y, 53 + RESET_VALUE);
        assert_eq!(recycler.recycler.gc.lock().unwrap().len(), 9);

        // allocate without expiration causes len to be reduced by 2
        let y: u64 = recycler.allocate("test_recycler1");
        assert_eq!(y, 52 + RESET_VALUE);
        assert_eq!(recycler.recycler.gc.lock().unwrap().len(), 8);
    }

    #[test]
    fn test_recycler_no_deadlock() {
        solana_logger::setup();

        let recycler =
            Recycler::<crate::cuda_runtime::PinnedVec<solana_sdk::packet::Packet>>::default();

        // create bunch of packets and drop at once to force enough inventory
        let packets = (0..=MAX_INVENTORY_COUNT_WITHOUT_EXPIRATION)
            .into_iter()
            .map(|_| recycler.allocate("me"))
            .collect::<Vec<_>>();
        drop(packets);

        recycler.recycler.gc.lock().unwrap().front_mut().unwrap().0 =
            Instant::now() - Duration::from_secs(7200);

        // this drops the first inventory item but shouldn't cause recycling of it!
        recycler.recycle_for_test(recycler.allocate("me"));
    }
}
