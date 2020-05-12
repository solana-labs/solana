use crate::{
    blockstore::Blockstore,
    shred::{Nonce, Shred, SIZE_OF_NONCE},
};
use solana_perf::packet::limited_deserialize;
use solana_sdk::{clock::Slot, packet::Packet};
use std::{io, net::SocketAddr};

pub fn repair_response_packet(
    blockstore: &Blockstore,
    slot: Slot,
    shred_index: u64,
    dest: &SocketAddr,
    nonce: Option<Nonce>,
) -> Option<Packet> {
    if Shred::is_nonce_unlocked(slot) && nonce.is_none()
        || !Shred::is_nonce_unlocked(slot) && nonce.is_some()
    {
        return None;
    }
    let shred = blockstore
        .get_data_shred(slot, shred_index)
        .expect("Blockstore could not get data shred");
    shred.map(|shred| repair_response_packet_from_shred(slot, shred, dest, nonce))
}

pub fn repair_response_packet_from_shred(
    slot: Slot,
    shred: Vec<u8>,
    dest: &SocketAddr,
    nonce: Nonce,
) -> Packet {
    let size_of_nonce = {
        if Shred::is_nonce_unlocked(slot) {
            assert!(nonce.is_some());
            SIZE_OF_NONCE
        } else {
            assert!(nonce.is_none());
            0
        }
    };
    let mut packet = Packet::default();
    packet.meta.size = shred.len() + size_of_nonce;
    packet.meta.set_addr(dest);
    packet.data[..shred.len()].copy_from_slice(&shred);
    let mut wr = io::Cursor::new(&mut packet.data[shred.len()..]);
    bincode::serialize_into(&mut wr, &nonce).expect("Buffer not large enough to fit nonce");
    packet
}

pub fn shred(buf: &[u8]) -> &[u8] {
    &buf[..buf.len() - SIZE_OF_NONCE]
}

pub fn nonce(buf: &[u8]) -> Option<Nonce> {
    limited_deserialize(&buf[buf.len() - SIZE_OF_NONCE..]).ok()
}
