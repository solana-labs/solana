// Cuda-only imports
#[cfg(feature = "cuda")]
use packet::PACKET_DATA_SIZE;
#[cfg(feature = "cuda")]
use std::mem::size_of;

// Non-cuda imports
#[cfg(not(feature = "cuda"))]
use rayon::prelude::*;
#[cfg(not(feature = "cuda"))]
use ring::signature;
#[cfg(not(feature = "cuda"))]
use untrusted;

// Shared imports
use packet::{Packet, SharedPackets};

pub const TX_OFFSET: usize = 4;
pub const SIGNED_DATA_OFFSET: usize = 112;
pub const SIG_OFFSET: usize = 8;
pub const PUB_KEY_OFFSET: usize = 80;

pub const SIG_SIZE: usize = 64;
pub const PUB_KEY_SIZE: usize = 32;

#[cfg(feature = "cuda")]
#[repr(C)]
struct Elems {
    elems: *const Packet,
    num: u32,
}

#[cfg(feature = "cuda")]
#[link(name = "cuda_verify_ed25519")]
extern "C" {
    fn ed25519_verify_many(
        vecs: *const Elems,
        num: u32,          //number of vecs
        message_size: u32, //size of each element inside the elems field of the vec
        public_key_offset: u32,
        signature_offset: u32,
        signed_message_offset: u32,
        signed_message_len_offset: u32,
        out: *mut u8, //combined length of all the items in vecs
    ) -> u32;
}

#[cfg(not(feature = "cuda"))]
fn verify_packet(packet: &Packet) -> u8 {
    let msg_start = TX_OFFSET + SIGNED_DATA_OFFSET;
    let sig_start = TX_OFFSET + SIG_OFFSET;
    let sig_end = sig_start + SIG_SIZE;
    let pub_key_start = TX_OFFSET + PUB_KEY_OFFSET;
    let pub_key_end = pub_key_start + PUB_KEY_SIZE;

    if packet.meta.size > msg_start {
        let msg_end = packet.meta.size;
        return if signature::verify(
            &signature::ED25519,
            untrusted::Input::from(&packet.data[pub_key_start..pub_key_end]),
            untrusted::Input::from(&packet.data[msg_start..msg_end]),
            untrusted::Input::from(&packet.data[sig_start..sig_end]),
        ).is_ok()
        {
            1
        } else {
            0
        };
    } else {
        return 0;
    }
}

#[cfg(not(feature = "cuda"))]
pub fn ed25519_verify(batches: &Vec<SharedPackets>) -> Vec<Vec<u8>> {
    let mut locks = Vec::new();
    let mut rvs = Vec::new();
    for packets in batches {
        locks.push(packets.read().unwrap());
    }

    for p in locks {
        let mut v = Vec::new();
        v.resize(p.packets.len(), 0);
        v = p.packets.par_iter().map(|x| verify_packet(x)).collect();
        rvs.push(v);
    }
    rvs
}

#[cfg(feature = "cuda")]
pub fn ed25519_verify(batches: &Vec<SharedPackets>) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    let mut elems = Vec::new();
    let mut locks = Vec::new();
    let mut rvs = Vec::new();

    for packets in batches {
        locks.push(packets.read().unwrap());
    }
    let mut num = 0;
    for p in locks {
        elems.push(Elems {
            elems: p.packets.as_ptr(),
            num: p.packets.len() as u32,
        });
        let mut v = Vec::new();
        v.resize(p.packets.len(), 0);
        rvs.push(v);
        num += p.packets.len();
    }
    out.resize(num, 0);
    trace!("Starting verify num packets: {}", num);
    trace!("elem len: {}", elems.len() as u32);
    trace!("packet sizeof: {}", size_of::<Packet>() as u32);
    trace!("pub key: {}", (TX_OFFSET + PUB_KEY_OFFSET) as u32);
    trace!("sig offset: {}", (TX_OFFSET + SIG_OFFSET) as u32);
    trace!("sign data: {}", (TX_OFFSET + SIGNED_DATA_OFFSET) as u32);
    trace!("len offset: {}", PACKET_DATA_SIZE as u32);
    unsafe {
        let res = ed25519_verify_many(
            elems.as_ptr(),
            elems.len() as u32,
            size_of::<Packet>() as u32,
            (TX_OFFSET + PUB_KEY_OFFSET) as u32,
            (TX_OFFSET + SIG_OFFSET) as u32,
            (TX_OFFSET + SIGNED_DATA_OFFSET) as u32,
            PACKET_DATA_SIZE as u32,
            out.as_mut_ptr(),
        );
        if res != 0 {
            trace!("RETURN!!!: {}", res);
        }
    }
    trace!("done verify");
    let mut num = 0;
    for vs in rvs.iter_mut() {
        for mut v in vs.iter_mut() {
            *v = out[num];
            if *v != 0 {
                trace!("VERIFIED PACKET!!!!!");
            }
            num += 1;
        }
    }
    rvs
}
