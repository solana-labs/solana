use crate::{
    blockstore::Blockstore,
    blockstore_db::Result,
    shred::{Nonce, SHRED_PAYLOAD_SIZE, SIZE_OF_NONCE},
};
use solana_perf::packet::limited_deserialize;
use solana_sdk::{clock::Slot, packet::Packet};
use std::{io, net::SocketAddr};

pub fn repair_response_packet(
    blockstore: &Blockstore,
    slot: Slot,
    shred_index: u64,
    dest: &SocketAddr,
    nonce: Nonce,
) -> Result<Option<Packet>> {
    let shred = blockstore.get_data_shred(slot, shred_index)?;
    Ok(shred.map(|shred| repair_response_packet_from_shred(shred, dest, nonce)))
}

pub fn repair_response_packet_from_shred(
    shred: Vec<u8>,
    dest: &SocketAddr,
    nonce: Nonce,
) -> Packet {
    assert!(shred.len() <= SHRED_PAYLOAD_SIZE);
    let mut packet = Packet::default();
    packet.meta.size = shred.len() + SIZE_OF_NONCE;
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
