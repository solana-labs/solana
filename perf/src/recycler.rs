use {
    rand::{thread_rng, Rng},
    std::sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc, Mutex, Weak,
    },
};

// A temporary burst in the workload can cause a large number of allocations,
// after which they will be recycled and still reside in memory. If the number
// of recycled objects stays above below limit for long, they will be deemed as
// redundant since they are not getting reused. The recycler will then shrink
// by releasing objects above this threshold. This limit aims to maintain a
// cushion against *normal* variations in the workload while bounding the
// number of redundant garbage collected objects after temporary bursts.
const RECYCLER_SHRINK_SIZE: usize = 1024;

// Lookback window for exponential moving averaging number of garbage collected
// objects in terms of number of allocations. The half-life of the decaying
// factor based on the window size defined below is 11356. This means a sample
// of gc.size() that is 11356 allocations ago has half of the weight as the most
// recent sample of gc.size() at current allocation.
const RECYCLER_SHRINK_WINDOW: usize = 16384;

#[derive(Debug, Default)]
struct RecyclerStats {
    total: AtomicUsize,
    reuse: AtomicUsize,
    freed: AtomicUsize,
    max_gc: AtomicUsize,
}

#[derive(Clone, Default)]
pub struct Recycler<T> {
    recycler: Arc<RecyclerX<T>>,
}

#[derive(Debug)]
pub struct RecyclerX<T> {
    gc: Mutex<Vec<T>>,
    stats: RecyclerStats,
    id: usize,
    // Shrink window times the exponential moving average size of gc.len().
    size_factor: AtomicUsize,
}

impl<T: Default> Default for RecyclerX<T> {
    fn default() -> RecyclerX<T> {
        let id = thread_rng().gen_range(0..1000);
        trace!("new recycler..{}", id);
        RecyclerX {
            gc: Mutex::default(),
            stats: RecyclerStats::default(),
            id,
            size_factor: AtomicUsize::default(),
        }
    }
}

#[cfg(RUSTC_WITH_SPECIALIZATION)]
impl solana_frozen_abi::abi_example::AbiExample
    for RecyclerX<crate::cuda_runtime::PinnedVec<solana_sdk::packet::Packet>>
{
    fn example() -> Self {
        Self::default()
    }
}

pub trait Reset {
    fn reset(&mut self);
    fn warm(&mut self, size_hint: usize);
    fn set_recycler(&mut self, recycler: Weak<RecyclerX<Self>>)
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

impl<T: Default + Reset + Sized> Recycler<T> {
    #[allow(clippy::needless_collect)]
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
        {
            const RECYCLER_SHRINK_WINDOW_HALF: usize = RECYCLER_SHRINK_WINDOW / 2;
            const RECYCLER_SHRINK_WINDOW_SUB_ONE: usize = RECYCLER_SHRINK_WINDOW - 1;
            let mut gc = self.recycler.gc.lock().unwrap();
            // Update the exponential moving average of gc.len().
            // The update equation is:
            //      a <- a * (n - 1) / n + x / n
            // To avoid floating point math, define b = n a:
            //      b <- b * (n - 1) / n + x
            // To make the remaining division to round (instead of truncate),
            // add n/2 to the numerator.
            // Effectively b (size_factor here) is an exponential moving
            // estimate of the "sum" of x (gc.len()) over the window as opposed
            // to the "average".
            self.recycler.size_factor.store(
                self.recycler
                    .size_factor
                    .load(Ordering::Acquire)
                    .saturating_mul(RECYCLER_SHRINK_WINDOW_SUB_ONE)
                    .saturating_add(RECYCLER_SHRINK_WINDOW_HALF)
                    .checked_div(RECYCLER_SHRINK_WINDOW)
                    .unwrap()
                    .saturating_add(gc.len()),
                Ordering::Release,
            );
            if let Some(mut x) = gc.pop() {
                self.recycler.stats.reuse.fetch_add(1, Ordering::Relaxed);
                x.reset();
                return x;
            }
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
}

impl<T: Default + Reset> RecyclerX<T> {
    pub fn recycle(&self, x: T) {
        let len = {
            let mut gc = self.gc.lock().expect("recycler lock in pub fn recycle");
            gc.push(x);
            const SIZE_FACTOR_AFTER_SHRINK: usize = RECYCLER_SHRINK_SIZE * RECYCLER_SHRINK_WINDOW;
            if gc.len() > RECYCLER_SHRINK_SIZE
                && self.size_factor.load(Ordering::Acquire) >= SIZE_FACTOR_AFTER_SHRINK
            {
                self.stats.freed.fetch_add(
                    gc.len().saturating_sub(RECYCLER_SHRINK_SIZE),
                    Ordering::Relaxed,
                );
                for mut x in gc.drain(RECYCLER_SHRINK_SIZE..) {
                    x.set_recycler(Weak::default());
                }
                self.size_factor
                    .store(SIZE_FACTOR_AFTER_SHRINK, Ordering::Release);
            }
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
        let freed = self.stats.freed.load(Ordering::Relaxed);
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
    use {super::*, crate::packet::PacketBatchRecycler, std::iter::repeat_with};

    impl Reset for u64 {
        fn reset(&mut self) {
            *self = 10;
        }
        fn warm(&mut self, _size_hint: usize) {}
        fn set_recycler(&mut self, _recycler: Weak<RecyclerX<Self>>) {}
    }

    #[test]
    fn test_recycler() {
        let recycler = Recycler::default();
        let mut y: u64 = recycler.allocate("test_recycler1");
        assert_eq!(y, 0);
        y = 20;
        let recycler2 = recycler.clone();
        recycler2.recycler.recycle(y);
        assert_eq!(recycler.recycler.gc.lock().unwrap().len(), 1);
        let z = recycler.allocate("test_recycler2");
        assert_eq!(z, 10);
        assert_eq!(recycler.recycler.gc.lock().unwrap().len(), 0);
    }

    #[test]
    fn test_recycler_shrink() {
        let mut rng = rand::thread_rng();
        let recycler = PacketBatchRecycler::default();
        // Allocate a burst of packets.
        const NUM_PACKETS: usize = RECYCLER_SHRINK_SIZE * 2;
        {
            let _packets: Vec<_> = repeat_with(|| recycler.allocate(""))
                .take(NUM_PACKETS)
                .collect();
        }
        assert_eq!(recycler.recycler.gc.lock().unwrap().len(), NUM_PACKETS);
        // Process a normal load of packets for a while.
        for _ in 0..RECYCLER_SHRINK_WINDOW / 16 {
            let count = rng.gen_range(1..128);
            let _packets: Vec<_> = repeat_with(|| recycler.allocate("")).take(count).collect();
        }
        // Assert that the gc size has shrinked.
        assert_eq!(
            recycler.recycler.gc.lock().unwrap().len(),
            RECYCLER_SHRINK_SIZE
        );
    }
}
