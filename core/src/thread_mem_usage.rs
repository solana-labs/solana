#[cfg(unix)]
use jemalloc_ctl::thread;

pub fn datapoint(_name: &'static str) {
    #[cfg(unix)]
    {
        let allocated = thread::allocatedp::mib().unwrap();
        let allocated = allocated.read().unwrap();
        let mem = allocated.get();
        solana_metrics::datapoint_debug!("thread-memory", (_name, mem as i64, i64));
    }
}

pub struct Allocatedp {
    #[cfg(unix)]
    allocated: thread::ThreadLocal<u64>,
}

impl Allocatedp {
    pub fn default() -> Self {
        #[cfg(unix)]
        {
            let allocated = thread::allocatedp::mib().unwrap();
            let allocated = allocated.read().unwrap();
            Self { allocated }
        }
        #[cfg(not(unix))]
        Self {}
    }

    pub fn get(&self) -> u64 {
        #[cfg(unix)]
        {
            self.allocated.get()
        }
        #[cfg(not(unix))]
        0
    }
}
