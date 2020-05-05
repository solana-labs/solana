use crate::{blockstore::Blockstore, blockstore_db::Result, shred::Nonce};
use serde::{Deserialize, Serialize};
use solana_sdk::{clock::Slot, packet::Packet};
use std::net::SocketAddr;

#[derive(Serialize, Deserialize, Debug, PartialEq)]
pub struct RepairResponse {
    pub shred: Vec<u8>,
    pub nonce: Nonce,
}

impl RepairResponse {
    pub fn new(shred: Vec<u8>, nonce: Nonce) -> Self {
        Self { shred, nonce }
    }

    pub fn repair_response_packet(
        blockstore: &Blockstore,
        slot: Slot,
        shred_index: u64,
        dest: &SocketAddr,
        nonce: Nonce,
    ) -> Result<Option<Packet>> {
        let shred = blockstore.get_data_shred(slot, shred_index)?;
        Ok(shred.map(|shred| Self::repair_response_packet_from_shred(shred, dest, nonce)))
    }

    pub fn repair_response_packet_from_shred(
        shred: Vec<u8>,
        dest: &SocketAddr,
        nonce: Nonce,
    ) -> Packet {
        let repair_response = RepairResponse::new(shred, nonce);
        Packet::from_data(dest, &repair_response)
    }
}
