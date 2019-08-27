//! @brief Example Rust-based BPF program tests loop iteration

#![no_std]
#![allow(unused_attributes)]

extern crate solana_sdk_bpf_utils;

pub struct Data<'a> {
    pub tone: u64,
    pub ttwo: u64,
    pub tthree: u64,
    pub tfour: u64,
    pub tfive: u32,
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
        _six: u64,
        _seven: u64,
        _eight: u64,
    ) -> Self  {
        Self {
            ten: data.tfive + five as u32 - 20,
        }
    }
}