#![allow(clippy::integer_arithmetic)]
pub mod measure;
pub mod thread_mem_usage;

#[cfg(target_os = "linux")]
extern crate jemallocator;

#[cfg(not(feature = "no-jemalloc"))]
#[cfg(target_os = "linux")]
#[global_allocator]
static ALLOC: jemallocator::Jemalloc = jemallocator::Jemalloc;
