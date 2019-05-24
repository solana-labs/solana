//! @brief Solana Rust-based BPF program utility functions and types

#![no_std]
#![feature(allocator_api)]
#![feature(alloc_error_handler)]

pub mod alloc;
pub mod entrypoint;
pub mod log;
pub mod panic;

#[global_allocator]
static A: alloc::Allocator = alloc::Allocator;
