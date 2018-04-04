use packet::SharedPackets;
use packet::{Packet, PACKET_DATA_SIZE};
use std::mem::size_of;

pub const TX_OFFSET: usize = 4;
pub const SIGNED_DATA_OFFSET: usize = 72;
pub const SIG_OFFSET: usize = 8;
pub const PUB_KEY_OFFSET: usize = 80;

#[repr(C)]
struct Elems {
    elems: *const Packet,
    num: u32,
}

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

pub fn ecdsa_verify(batches: &Vec<SharedPackets>) -> Vec<Vec<u8>> {
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
    //println!("Starting verify num packets: {}", num);
    //println!("elem len: {}", elems.len() as u32);
    //println!("packet sizeof: {}", size_of::<Packet>() as u32);
    //println!("pub key: {}", (TX_OFFSET + PUB_KEY_OFFSET) as u32);
    //println!("sig offset: {}", (TX_OFFSET + SIG_OFFSET) as u32);
    //println!("sign data: {}", (TX_OFFSET + SIGNED_DATA_OFFSET) as u32);
    //println!("len offset: {}", PACKET_DATA_SIZE as u32);
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
            //    println!("RETURN!!!: {}", res);
        }
    }
    //println!("done verify");
    let mut num = 0;
    for vs in rvs.iter_mut() {
        for mut v in vs.iter_mut() {
            *v = out[num];
            if *v != 0 {
                //    println!("VERIFIED PACKET!!!!!");
            }
            num += 1;
        }
    }
    rvs
}
