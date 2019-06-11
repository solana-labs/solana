//! @brief Solana Rust-based BPF program utility functions and types

#![no_std]
#![feature(allocator_api)]
#![feature(alloc_error_handler)]
#![feature(panic_info_message)]
#![feature(compiler_builtins_lib)]

extern crate compiler_builtins;

pub mod allocator;
pub mod entrypoint;
pub mod log;
pub mod panic;

#[global_allocator]
static A: allocator::Allocator = allocator::Allocator;
