#[cfg(not(feature = "no-jemalloc"))]
#[cfg(target_os = "linux")]
use jemalloc_ctl::thread;

pub fn datapoint(_name: &'static str) {
    #[cfg(not(feature = "no-jemalloc"))]
    #[cfg(target_os = "linux")]
    {
        let allocated = thread::allocatedp::mib().unwrap();
        let allocated = allocated.read().unwrap();
        let mem = allocated.get();
        solana_metrics::datapoint_debug!("thread-memory", (_name, mem as i64, i64));
    }
}

pub struct Allocatedp {
    #[cfg(not(feature = "no-jemalloc"))]
    #[cfg(target_os = "linux")]
    allocated: thread::ThreadLocal<u64>,
}

impl Allocatedp {
    pub fn default() -> Self {
        #[cfg(not(feature = "no-jemalloc"))]
        #[cfg(target_os = "linux")]
        {
            let allocated = thread::allocatedp::mib().unwrap();
            let allocated = allocated.read().unwrap();
            Self { allocated }
        }
        #[cfg(any(feature = "no-jemalloc", not(target_os = "linux")))]
        Self {}
    }

    /// Return current thread heap usage
    pub fn get(&self) -> u64 {
        #[cfg(not(feature = "no-jemalloc"))]
        #[cfg(target_os = "linux")]
        {
            self.allocated.get()
        }
        #[cfg(any(feature = "no-jemalloc", not(target_os = "linux")))]
        0
    }

    /// Return the difference in thread heap usage since a previous `get()`
    pub fn since(&self, previous: u64) -> i64 {
        self.get() as i64 - previous as i64
    }
}
