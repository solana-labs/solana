#![allow(clippy::integer_arithmetic)]
pub mod measure;
pub mod thread_mem_usage;

// #[cfg(all(unix, not(all(target_os = "macos", target_arch="aarch64-app-darwin"))))]
// #[cfg(unix)]
extern crate jemallocator;

#[cfg(not(feature = "no-jemalloc"))]
#[cfg(all(unix, not(all(target_os = "macos", target_arch="aarch64-apple-darwin"))))]
#[global_allocator]
static ALLOC: jemallocator::Jemalloc = jemallocator::Jemalloc;
