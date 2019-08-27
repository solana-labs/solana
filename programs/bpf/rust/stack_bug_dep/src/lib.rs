//! @brief Example Rust-based BPF program tests loop iteration

#![no_std]
#![allow(unused_attributes)]

extern crate solana_sdk_bpf_utils;

pub struct Data<'a> {
    pub five: u32,
    pub array: &'a [u8],
}

pub struct TestDep {
    pub ten: u32,
}
impl<'a> TestDep {
    pub fn new(
        data: &Data<'a>,
        _one: u64,
        _two: u64,
        _three: u64,
        _four: u64,
        five: u64,
    ) -> Self  {
        Self {
            ten: data.five + five as u32,
        }
    }
}