pub mod measure;
pub mod thread_mem_usage;

#[cfg(unix)]
extern crate jemallocator;

#[cfg(unix)]
#[global_allocator]
static ALLOC: jemallocator::Jemalloc = jemallocator::Jemalloc;
