//! @brief Example Rust-based BPF program tests loop iteration

#![no_std]
#![allow(unused_attributes)]

extern crate alloc;
#[cfg(not(test))]
extern crate solana_sdk_bpf_no_std;
extern crate solana_sdk_bpf_utils;

use alloc::vec::Vec;
use solana_sdk_bpf_utils::entrypoint::{SolPubkey};

pub struct InitPollData<'a> {
    pub timeout: u32,
    pub header_len: u32,
    pub header: &'a [u8],
    pub option_a_len: u32,
    pub option_a: &'a [u8],
    pub option_b_len: u32,
    pub option_b: &'a [u8],
}

pub struct PollData<'a> {
    pub creator_key: &'a SolPubkey,
    pub last_block: u64,
    pub header_len: u32,
    pub header: &'a [u8],
    pub option_a: PollOptionData<'a>,
    pub option_b: PollOptionData<'a>,
}

impl<'a> PollData<'a> {
    pub fn length(&self) -> usize {
        (32 + 8 + 4 + self.header_len) as usize + self.option_a.length() + self.option_b.length()
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(self.length());
        bytes.extend_from_slice(self.creator_key);
        bytes.extend_from_slice(&self.last_block.to_be_bytes());
        bytes.extend_from_slice(&self.header_len.to_be_bytes());
        bytes.extend_from_slice(self.header);
        bytes.extend(self.option_a.to_bytes().into_iter());
        bytes.extend(self.option_b.to_bytes().into_iter());
        bytes
    }

    pub fn init(
        init: InitPollData<'a>,
        creator_key: &'a SolPubkey,
        tally_a_key: &'a SolPubkey,
        tally_b_key: &'a SolPubkey,
        slot: u64,
    ) -> Self {
        assert_eq!(init.timeout, 10);
        Self {
            creator_key,
            last_block: slot + init.timeout as u64,
            header_len: init.header_len,
            header: init.header,
            option_a: PollOptionData {
                text_len: init.option_a_len,
                text: init.option_a,
                tally_key: tally_a_key,
                quantity: 0,
            },
            option_b: PollOptionData {
                text_len: init.option_b_len,
                text: init.option_b,
                tally_key: tally_b_key,
                quantity: 0,
            },
        }
    }
}

pub struct PollOptionData<'a> {
    pub text_len: u32,
    pub text: &'a [u8],
    pub tally_key: &'a SolPubkey,
    pub quantity: u64,
}

impl<'a> PollOptionData<'a> {
    pub fn length(&self) -> usize {
        (4 + self.text_len + 32 + 8) as usize
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(self.length());
        bytes.extend_from_slice(&self.text_len.to_be_bytes());
        bytes.extend_from_slice(self.text);
        bytes.extend_from_slice(self.tally_key);
        bytes.extend_from_slice(&self.quantity.to_be_bytes());
        bytes
    }
}
