use crate::clock::{Epoch, GENESIS_EPOCH};
use crate::fee_calculator::FeeConfig;
use crate::genesis_config::ClusterType;
use digest::Digest;
use serde_derive::{Deserialize, Serialize};

pub fn get_fee_config(cluster_type: ClusterType, epoch: Epoch) -> Option<FeeConfig> {
    Some(FeeConfig {
        is_secp256k1_enabled: is_enabled(cluster_type, epoch),
    })
}

pub fn is_enabled_epoch(cluster_type: ClusterType) -> Epoch {
    match cluster_type {
        ClusterType::Development => GENESIS_EPOCH,
        ClusterType::Testnet => u64::MAX,
        ClusterType::MainnetBeta => u64::MAX,
        ClusterType::Devnet => u64::MAX,
    }
}

pub fn is_enabled(cluster_type: ClusterType, epoch: Epoch) -> bool {
    epoch >= is_enabled_epoch(cluster_type)
}

#[derive(Debug)]
pub enum Secp256k1Error {
    InvalidSignature,
    InvalidRecoveryId,
    InvalidDataOffsets,
    InvalidInstructionDataSize,
}

pub const HASHED_PUBKEY_SERIALIZED_SIZE: usize = 20;
pub const SIGNATURE_SERIALIZED_SIZE: usize = 64;
pub const SIGNATURE_OFFSETS_SERIALIZED_SIZE: usize = 11;

pub fn construct_eth_pubkey(pubkey: &secp256k1::PublicKey) -> [u8; HASHED_PUBKEY_SERIALIZED_SIZE] {
    let mut addr = [0u8; HASHED_PUBKEY_SERIALIZED_SIZE];
    addr.copy_from_slice(&sha3::Keccak256::digest(&pubkey.serialize()[1..])[12..]);
    assert_eq!(addr.len(), HASHED_PUBKEY_SERIALIZED_SIZE);
    addr
}

#[derive(Default, Serialize, Deserialize, Debug)]
pub struct SecpSignatureOffsets {
    pub signature_offset: u16, // offset to [signature,recovery_id] of 64+1 bytes
    pub signature_instruction_index: u8,
    pub eth_address_offset: u16, // offset to eth_address of 20 bytes
    pub eth_address_instruction_index: u8,
    pub message_data_offset: u16, // offset to start of message data
    pub message_data_size: u16,   // size of message data
    pub message_instruction_index: u8,
}

fn get_data_slice<'a>(
    instruction_datas: &'a [&[u8]],
    instruction_index: u8,
    offset_start: u16,
    size: usize,
) -> Result<&'a [u8], Secp256k1Error> {
    let signature_index = instruction_index as usize;
    if signature_index > instruction_datas.len() {
        return Err(Secp256k1Error::InvalidDataOffsets);
    }
    let signature_instruction = &instruction_datas[signature_index];
    let start = offset_start as usize;
    let end = start + size;
    if end > signature_instruction.len() {
        return Err(Secp256k1Error::InvalidSignature);
    }

    Ok(&instruction_datas[signature_index][start..end])
}

pub fn verify_eth_addresses(
    data: &[u8],
    instruction_datas: &[&[u8]],
) -> Result<(), Secp256k1Error> {
    if data.is_empty() {
        return Err(Secp256k1Error::InvalidInstructionDataSize);
    }
    let count = data[0] as usize;
    let expected_data_size = 1 + count * SIGNATURE_OFFSETS_SERIALIZED_SIZE;
    if data.len() < expected_data_size {
        return Err(Secp256k1Error::InvalidInstructionDataSize);
    }
    for i in 0..count {
        let start = 1 + i * SIGNATURE_OFFSETS_SERIALIZED_SIZE;
        let end = start + SIGNATURE_OFFSETS_SERIALIZED_SIZE;

        let offsets: SecpSignatureOffsets = bincode::deserialize(&data[start..end])
            .map_err(|_| Secp256k1Error::InvalidSignature)?;

        // Parse out signature
        let signature_index = offsets.signature_instruction_index as usize;
        if signature_index > instruction_datas.len() {
            return Err(Secp256k1Error::InvalidInstructionDataSize);
        }
        let signature_instruction = instruction_datas[signature_index];
        let sig_start = offsets.signature_offset as usize;
        let sig_end = sig_start + SIGNATURE_SERIALIZED_SIZE;
        if sig_end >= signature_instruction.len() {
            return Err(Secp256k1Error::InvalidSignature);
        }
        let signature =
            secp256k1::Signature::parse_slice(&signature_instruction[sig_start..sig_end])
                .map_err(|_| Secp256k1Error::InvalidSignature)?;

        let recovery_id = secp256k1::RecoveryId::parse(signature_instruction[sig_end])
            .map_err(|_| Secp256k1Error::InvalidRecoveryId)?;

        // Parse out pubkey
        let eth_address_slice = get_data_slice(
            &instruction_datas,
            offsets.eth_address_instruction_index,
            offsets.eth_address_offset,
            HASHED_PUBKEY_SERIALIZED_SIZE,
        )?;

        // Parse out message
        let message_slice = get_data_slice(
            &instruction_datas,
            offsets.message_instruction_index,
            offsets.message_data_offset,
            offsets.message_data_size as usize,
        )?;

        let mut hasher = sha3::Keccak256::new();
        hasher.update(message_slice);
        let message_hash = hasher.finalize();

        let pubkey = secp256k1::recover(
            &secp256k1::Message::parse_slice(&message_hash).unwrap(),
            &signature,
            &recovery_id,
        )
        .map_err(|_| Secp256k1Error::InvalidSignature)?;
        let eth_address = construct_eth_pubkey(&pubkey);

        if eth_address_slice != eth_address {
            return Err(Secp256k1Error::InvalidSignature);
        }
    }
    Ok(())
}
