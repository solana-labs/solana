use jemalloc_ctl::thread;
use solana_metrics::datapoint_debug;

pub fn datapoint(name: &'static str) {
    let allocated = thread::allocatedp::mib().unwrap();
    let allocated = allocated.read().unwrap();
    let mem = allocated.get();
    datapoint_debug!("thread-memory", (name, mem as i64, i64));
}
