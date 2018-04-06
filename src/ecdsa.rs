use packet::{Packet, SharedPackets};
use transaction::{PUB_KEY_OFFSET, SIGNED_DATA_OFFSET, SIG_OFFSET};
use std::mem::size_of;

pub const TX_OFFSET: usize = 4;

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
    use ring::signature;
    use untrusted;
    use signature::{PublicKey, Signature};

    let msg_start = TX_OFFSET + SIGNED_DATA_OFFSET;
    let sig_start = TX_OFFSET + SIG_OFFSET;
    let sig_end = sig_start + size_of::<Signature>();
    let pub_key_start = TX_OFFSET + PUB_KEY_OFFSET;
    let pub_key_end = pub_key_start + size_of::<PublicKey>();

    if packet.meta.size <= msg_start {
        return 0;
    }

    let msg_end = packet.meta.size;
    signature::verify(
        &signature::ED25519,
        untrusted::Input::from(&packet.data[pub_key_start..pub_key_end]),
        untrusted::Input::from(&packet.data[msg_start..msg_end]),
        untrusted::Input::from(&packet.data[sig_start..sig_end]),
    ).is_ok() as u8
}

#[cfg(not(feature = "cuda"))]
pub fn ed25519_verify(batches: &Vec<SharedPackets>) -> Vec<Vec<u8>> {
    use rayon::prelude::*;

    batches
        .into_par_iter()
        .map(|p| {
            p.read()
                .unwrap()
                .packets
                .par_iter()
                .map(verify_packet)
                .collect()
        })
        .collect()
}

#[cfg(feature = "cuda")]
pub fn ed25519_verify(batches: &Vec<SharedPackets>) -> Vec<Vec<u8>> {
    use packet::PACKET_DATA_SIZE;

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
