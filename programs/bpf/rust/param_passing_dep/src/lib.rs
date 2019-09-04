//! @brief Example Rust-based BPF program tests loop iteration

extern crate solana_sdk_bpf_utils;

pub struct Data<'a> {
    pub twentyone: u64,
    pub twentytwo: u64,
    pub twentythree: u64,
    pub twentyfour: u64,
    pub twentyfive: u32,
    pub array: &'a [u8],
}

pub struct TestDep {
    pub thirty: u32,
}
impl<'a> TestDep {
    pub fn new(data: &Data<'a>, _one: u64, _two: u64, _three: u64, _four: u64, five: u64) -> Self {
        Self {
            thirty: data.twentyfive + five as u32,
        }
    }
}
