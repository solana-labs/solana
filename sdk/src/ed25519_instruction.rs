#![cfg(feature = "full")]

use crate::{decode_error::DecodeError, instruction::Instruction};
use bytemuck::{bytes_of, Pod, Zeroable};
use ed25519_dalek::{ed25519::signature::Signature, Signer, Verifier};
use thiserror::Error;

#[derive(Error, Debug, Clone, PartialEq)]
pub enum Ed25519Error {
    #[error("ed25519 public key is not valid")]
    InvalidPublicKey,
    #[error("ed25519 signature is not valid")]
    InvalidSignature,
    #[error("offset not valid")]
    InvalidDataOffsets,
    #[error("instruction is incorrect size")]
    InvalidInstructionDataSize,
}

impl<T> DecodeError<T> for Ed25519Error {
    fn type_of() -> &'static str {
        "Ed25519Error"
    }
}

pub const PUBKEY_SERIALIZED_SIZE: usize = 32;
pub const SIGNATURE_SERIALIZED_SIZE: usize = 64;
pub const SIGNATURE_OFFSETS_SERIALIZED_SIZE: usize = 14;
// bytemuck requires structures to be aligned
pub const SIGNATURE_OFFSETS_START: usize = 2;
pub const DATA_START: usize = SIGNATURE_OFFSETS_SERIALIZED_SIZE + SIGNATURE_OFFSETS_START;

#[derive(Default, Debug, Copy, Clone, Zeroable, Pod)]
#[repr(C)]
pub struct Ed25519SignatureOffsets {
    signature_offset: u16,             // offset to ed25519 signature of 64 bytes
    signature_instruction_index: u16,  // instruction index to find signature
    public_key_offset: u16,            // offset to public key of 32 bytes
    public_key_instruction_index: u16, // instruction index to find public key
    message_data_offset: u16,          // offset to start of message data
    message_data_size: u16,            // size of message data
    message_instruction_index: u16,    // index of instruction data to get message data
}

pub fn new_ed25519_instruction(keypair: &ed25519_dalek::Keypair, message: &[u8]) -> Instruction {
    let signature = keypair.sign(message).to_bytes();
    let pubkey = keypair.public.to_bytes();

    assert_eq!(pubkey.len(), PUBKEY_SERIALIZED_SIZE);
    assert_eq!(signature.len(), SIGNATURE_SERIALIZED_SIZE);

    let mut instruction_data = Vec::with_capacity(
        DATA_START
            .saturating_add(SIGNATURE_SERIALIZED_SIZE)
            .saturating_add(PUBKEY_SERIALIZED_SIZE)
            .saturating_add(message.len()),
    );

    let num_signatures: u8 = 1;
    let public_key_offset = DATA_START;
    let signature_offset = public_key_offset.saturating_add(PUBKEY_SERIALIZED_SIZE);
    let message_data_offset = signature_offset.saturating_add(SIGNATURE_SERIALIZED_SIZE);

    // add padding byte so that offset structure is aligned
    instruction_data.extend_from_slice(bytes_of(&[num_signatures, 0]));

    let offsets = Ed25519SignatureOffsets {
        signature_offset: signature_offset as u16,
        signature_instruction_index: 0,
        public_key_offset: public_key_offset as u16,
        public_key_instruction_index: 0,
        message_data_offset: message_data_offset as u16,
        message_data_size: message.len() as u16,
        message_instruction_index: 0,
    };

    instruction_data.extend_from_slice(bytes_of(&offsets));

    debug_assert_eq!(instruction_data.len(), public_key_offset);

    instruction_data.extend_from_slice(&pubkey);

    debug_assert_eq!(instruction_data.len(), signature_offset);

    instruction_data.extend_from_slice(&signature);

    debug_assert_eq!(instruction_data.len(), message_data_offset);

    instruction_data.extend_from_slice(message);

    Instruction {
        program_id: solana_sdk::ed25519_program::id(),
        accounts: vec![],
        data: instruction_data,
    }
}

pub fn verify_signatures(data: &[u8], instruction_datas: &[&[u8]]) -> Result<(), Ed25519Error> {
    if data.len() < SIGNATURE_OFFSETS_START {
        return Err(Ed25519Error::InvalidInstructionDataSize);
    }
    let num_signatures = data[0] as usize;
    if num_signatures == 0 && data.len() > SIGNATURE_OFFSETS_START {
        return Err(Ed25519Error::InvalidInstructionDataSize);
    }
    let expected_data_size = num_signatures
        .saturating_mul(SIGNATURE_OFFSETS_SERIALIZED_SIZE)
        .saturating_add(SIGNATURE_OFFSETS_START);
    if data.len() < expected_data_size {
        return Err(Ed25519Error::InvalidInstructionDataSize);
    }
    for i in 0..num_signatures {
        let start = i
            .saturating_mul(SIGNATURE_OFFSETS_SERIALIZED_SIZE)
            .saturating_add(SIGNATURE_OFFSETS_START);
        let end = start.saturating_add(SIGNATURE_OFFSETS_SERIALIZED_SIZE);

        // bytemuck wants structures aligned
        let offsets: &Ed25519SignatureOffsets = bytemuck::try_from_bytes(&data[start..end])
            .map_err(|_| Ed25519Error::InvalidDataOffsets)?;

        // Parse out signature
        let signature_index = offsets.signature_instruction_index as usize;
        if signature_index >= instruction_datas.len() {
            return Err(Ed25519Error::InvalidDataOffsets);
        }
        let signature_instruction = instruction_datas[signature_index];
        let sig_start = offsets.signature_offset as usize;
        let sig_end = sig_start.saturating_add(SIGNATURE_SERIALIZED_SIZE);
        if sig_end >= signature_instruction.len() {
            return Err(Ed25519Error::InvalidDataOffsets);
        }

        let signature =
            ed25519_dalek::Signature::from_bytes(&signature_instruction[sig_start..sig_end])
                .map_err(|_| Ed25519Error::InvalidSignature)?;

        // Parse out pubkey
        let pubkey = get_data_slice(
            instruction_datas,
            offsets.public_key_instruction_index,
            offsets.public_key_offset,
            PUBKEY_SERIALIZED_SIZE,
        )?;

        let publickey = ed25519_dalek::PublicKey::from_bytes(pubkey)
            .map_err(|_| Ed25519Error::InvalidPublicKey)?;

        // Parse out message
        let message = get_data_slice(
            instruction_datas,
            offsets.message_instruction_index,
            offsets.message_data_offset,
            offsets.message_data_size as usize,
        )?;

        publickey
            .verify(message, &signature)
            .map_err(|_| Ed25519Error::InvalidSignature)?;
    }
    Ok(())
}

fn get_data_slice<'a>(
    instruction_datas: &'a [&[u8]],
    instruction_index: u16,
    offset_start: u16,
    size: usize,
) -> Result<&'a [u8], Ed25519Error> {
    let signature_index = instruction_index as usize;
    if signature_index >= instruction_datas.len() {
        return Err(Ed25519Error::InvalidDataOffsets);
    }
    let signature_instruction = &instruction_datas[signature_index];
    let start = offset_start as usize;
    let end = start.saturating_add(size);
    if end > signature_instruction.len() {
        return Err(Ed25519Error::InvalidDataOffsets);
    }

    Ok(&instruction_datas[signature_index][start..end])
}

#[cfg(test)]
pub mod test {
    use super::*;

    fn test_case(
        num_signatures: u16,
        offsets: &Ed25519SignatureOffsets,
    ) -> Result<(), Ed25519Error> {
        assert_eq!(
            bytemuck::bytes_of(offsets).len(),
            SIGNATURE_OFFSETS_SERIALIZED_SIZE
        );

        let mut instruction_data = vec![0u8; DATA_START];
        instruction_data[0..SIGNATURE_OFFSETS_START].copy_from_slice(bytes_of(&num_signatures));
        instruction_data[SIGNATURE_OFFSETS_START..DATA_START].copy_from_slice(bytes_of(offsets));

        verify_signatures(&instruction_data, &[&[0u8; 100]])
    }

    #[test]
    fn test_invalid_offsets() {
        solana_logger::setup();

        let mut instruction_data = vec![0u8; DATA_START];
        let offsets = Ed25519SignatureOffsets::default();
        instruction_data[0..SIGNATURE_OFFSETS_START].copy_from_slice(bytes_of(&1u16));
        instruction_data[SIGNATURE_OFFSETS_START..DATA_START].copy_from_slice(bytes_of(&offsets));
        instruction_data.truncate(instruction_data.len() - 1);

        assert_eq!(
            verify_signatures(&instruction_data, &[&[0u8; 100]]),
            Err(Ed25519Error::InvalidInstructionDataSize)
        );

        let offsets = Ed25519SignatureOffsets {
            signature_instruction_index: 1,
            ..Ed25519SignatureOffsets::default()
        };
        assert_eq!(
            test_case(1, &offsets),
            Err(Ed25519Error::InvalidDataOffsets)
        );

        let offsets = Ed25519SignatureOffsets {
            message_instruction_index: 1,
            ..Ed25519SignatureOffsets::default()
        };
        assert_eq!(
            test_case(1, &offsets),
            Err(Ed25519Error::InvalidDataOffsets)
        );

        let offsets = Ed25519SignatureOffsets {
            public_key_instruction_index: 1,
            ..Ed25519SignatureOffsets::default()
        };
        assert_eq!(
            test_case(1, &offsets),
            Err(Ed25519Error::InvalidDataOffsets)
        );
    }

    #[test]
    fn test_message_data_offsets() {
        let offsets = Ed25519SignatureOffsets {
            message_data_offset: 99,
            message_data_size: 1,
            ..Ed25519SignatureOffsets::default()
        };
        assert_eq!(test_case(1, &offsets), Err(Ed25519Error::InvalidSignature));

        let offsets = Ed25519SignatureOffsets {
            message_data_offset: 100,
            message_data_size: 1,
            ..Ed25519SignatureOffsets::default()
        };
        assert_eq!(
            test_case(1, &offsets),
            Err(Ed25519Error::InvalidDataOffsets)
        );

        let offsets = Ed25519SignatureOffsets {
            message_data_offset: 100,
            message_data_size: 1000,
            ..Ed25519SignatureOffsets::default()
        };
        assert_eq!(
            test_case(1, &offsets),
            Err(Ed25519Error::InvalidDataOffsets)
        );

        let offsets = Ed25519SignatureOffsets {
            message_data_offset: std::u16::MAX,
            message_data_size: std::u16::MAX,
            ..Ed25519SignatureOffsets::default()
        };
        assert_eq!(
            test_case(1, &offsets),
            Err(Ed25519Error::InvalidDataOffsets)
        );
    }

    #[test]
    fn test_pubkey_offset() {
        let offsets = Ed25519SignatureOffsets {
            public_key_offset: std::u16::MAX,
            ..Ed25519SignatureOffsets::default()
        };
        assert_eq!(
            test_case(1, &offsets),
            Err(Ed25519Error::InvalidDataOffsets)
        );

        let offsets = Ed25519SignatureOffsets {
            public_key_offset: 100 - PUBKEY_SERIALIZED_SIZE as u16 + 1,
            ..Ed25519SignatureOffsets::default()
        };
        assert_eq!(
            test_case(1, &offsets),
            Err(Ed25519Error::InvalidDataOffsets)
        );
    }

    #[test]
    fn test_signature_offset() {
        let offsets = Ed25519SignatureOffsets {
            signature_offset: std::u16::MAX,
            ..Ed25519SignatureOffsets::default()
        };
        assert_eq!(
            test_case(1, &offsets),
            Err(Ed25519Error::InvalidDataOffsets)
        );

        let offsets = Ed25519SignatureOffsets {
            signature_offset: 100 - SIGNATURE_SERIALIZED_SIZE as u16 + 1,
            ..Ed25519SignatureOffsets::default()
        };
        assert_eq!(
            test_case(1, &offsets),
            Err(Ed25519Error::InvalidDataOffsets)
        );
    }
}
