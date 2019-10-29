use rand::{thread_rng, Rng};
use std::sync::atomic::AtomicBool;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

#[derive(Debug, Default)]
struct RecyclerStats {
    total: AtomicUsize,
    reuse: AtomicUsize,
    max_gc: AtomicUsize,
}

#[derive(Debug)]
pub struct Recycler<T> {
    gc: Arc<Mutex<Vec<T>>>,
    stats: Arc<RecyclerStats>,
    id: usize,
}

impl<T: Default> Default for Recycler<T> {
    fn default() -> Recycler<T> {
        let id = thread_rng().gen_range(0, 1000);
        trace!("new recycler..{}", id);
        Recycler {
            gc: Arc::new(Mutex::new(vec![])),
            stats: Arc::new(RecyclerStats::default()),
            id,
        }
    }
}

impl<T: Default> Clone for Recycler<T> {
    fn clone(&self) -> Recycler<T> {
        Recycler {
            gc: self.gc.clone(),
            stats: self.stats.clone(),
            id: self.id,
        }
    }
}

pub trait Reset {
    fn reset(&mut self);
    fn warm(&mut self, size_hint: usize);
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

impl<T: Default + Reset> Recycler<T> {
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
            warmed_items.into_iter().for_each(|i| new.recycle(i));
        }
        new
    }

    pub fn allocate(&self, name: &'static str) -> T {
        let new = self
            .gc
            .lock()
            .expect("recycler lock in pb fn allocate")
            .pop();

        if let Some(mut x) = new {
            self.stats.reuse.fetch_add(1, Ordering::Relaxed);
            x.reset();
            return x;
        }

        trace!(
            "allocating new: total {} {:?} id: {} reuse: {} max_gc: {}",
            self.stats.total.fetch_add(1, Ordering::Relaxed),
            name,
            self.id,
            self.stats.reuse.load(Ordering::Relaxed),
            self.stats.max_gc.load(Ordering::Relaxed),
        );

        T::default()
    }

    pub fn recycle(&self, x: T) {
        let len = {
            let mut gc = self.gc.lock().expect("recycler lock in pub fn recycle");
            gc.push(x);
            gc.len()
        };

        let max_gc = self.stats.max_gc.load(Ordering::Relaxed);
        if len > max_gc {
            // this is not completely accurate, but for most cases should be fine.
            self.stats
                .max_gc
                .compare_and_swap(max_gc, len, Ordering::Relaxed);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    impl Reset for u64 {
        fn reset(&mut self) {
            *self = 10;
        }
        fn warm(&mut self, _size_hint: usize) {}
    }

    #[test]
    fn test_recycler() {
        let recycler = Recycler::default();
        let mut y: u64 = recycler.allocate("test_recycler1");
        assert_eq!(y, 0);
        y = 20;
        let recycler2 = recycler.clone();
        recycler2.recycle(y);
        assert_eq!(recycler.gc.lock().unwrap().len(), 1);
        let z = recycler.allocate("test_recycler2");
        assert_eq!(z, 10);
        assert_eq!(recycler.gc.lock().unwrap().len(), 0);
    }
}
