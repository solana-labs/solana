pub use implementation::AtomicU64;

#[cfg(target_pointer_width = "64")]
mod implementation {
    use std::sync::atomic;

    pub struct AtomicU64(atomic::AtomicU64);

    impl AtomicU64 {
        pub const fn new(initial: u64) -> Self {
            Self(atomic::AtomicU64::new(initial))
        }

        pub fn fetch_add(&self, v: u64) -> u64 {
            self.0.fetch_add(v, atomic::Ordering::Relaxed)
        }
    }
}

#[cfg(not(target_pointer_width = "64"))]
mod implementation {
    use parking_lot::{const_mutex, Mutex};

    pub struct AtomicU64(Mutex<u64>);

    impl AtomicU64 {
        pub const fn new(initial: u64) -> Self {
            Self(const_mutex(initial))
        }

        pub fn fetch_add(&self, v: u64) -> u64 {
            let mut lock = self.0.lock();
            let i = *lock;
            *lock = i + v;
            i
        }
    }
}
