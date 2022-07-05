#![cfg(feature = "full")]

use {
    crate::{
        feature_set::{
            libsecp256k1_0_5_upgrade_enabled, libsecp256k1_fail_on_bad_count,
            libsecp256k1_fail_on_bad_count2, FeatureSet,
        },
        instruction::Instruction,
        precompiles::PrecompileError,
    },
    digest::Digest,
    serde_derive::{Deserialize, Serialize},
    std::sync::Arc,
};

pub const HASHED_PUBKEY_SERIALIZED_SIZE: usize = 20;
pub const SIGNATURE_SERIALIZED_SIZE: usize = 64;
pub const SIGNATURE_OFFSETS_SERIALIZED_SIZE: usize = 11;
pub const DATA_START: usize = SIGNATURE_OFFSETS_SERIALIZED_SIZE + 1;

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

pub fn new_secp256k1_instruction(
    priv_key: &libsecp256k1::SecretKey,
    message_arr: &[u8],
) -> Instruction {
    let secp_pubkey = libsecp256k1::PublicKey::from_secret_key(priv_key);
    let eth_pubkey = construct_eth_pubkey(&secp_pubkey);
    let mut hasher = sha3::Keccak256::new();
    hasher.update(&message_arr);
    let message_hash = hasher.finalize();
    let mut message_hash_arr = [0u8; 32];
    message_hash_arr.copy_from_slice(message_hash.as_slice());
    let message = libsecp256k1::Message::parse(&message_hash_arr);
    let (signature, recovery_id) = libsecp256k1::sign(&message, priv_key);
    let signature_arr = signature.serialize();
    assert_eq!(signature_arr.len(), SIGNATURE_SERIALIZED_SIZE);

    let mut instruction_data = vec![];
    instruction_data.resize(
        DATA_START
            .saturating_add(eth_pubkey.len())
            .saturating_add(signature_arr.len())
            .saturating_add(message_arr.len())
            .saturating_add(1),
        0,
    );
    let eth_address_offset = DATA_START;
    instruction_data[eth_address_offset..eth_address_offset.saturating_add(eth_pubkey.len())]
        .copy_from_slice(&eth_pubkey);

    let signature_offset = DATA_START.saturating_add(eth_pubkey.len());
    instruction_data[signature_offset..signature_offset.saturating_add(signature_arr.len())]
        .copy_from_slice(&signature_arr);

    instruction_data[signature_offset.saturating_add(signature_arr.len())] =
        recovery_id.serialize();

    let message_data_offset = signature_offset
        .saturating_add(signature_arr.len())
        .saturating_add(1);
    instruction_data[message_data_offset..].copy_from_slice(message_arr);

    let num_signatures = 1;
    instruction_data[0] = num_signatures;
    let offsets = SecpSignatureOffsets {
        signature_offset: signature_offset as u16,
        signature_instruction_index: 0,
        eth_address_offset: eth_address_offset as u16,
        eth_address_instruction_index: 0,
        message_data_offset: message_data_offset as u16,
        message_data_size: message_arr.len() as u16,
        message_instruction_index: 0,
    };
    let writer = std::io::Cursor::new(&mut instruction_data[1..DATA_START]);
    bincode::serialize_into(writer, &offsets).unwrap();

    Instruction {
        program_id: solana_sdk::secp256k1_program::id(),
        accounts: vec![],
        data: instruction_data,
    }
}

pub fn construct_eth_pubkey(
    pubkey: &libsecp256k1::PublicKey,
) -> [u8; HASHED_PUBKEY_SERIALIZED_SIZE] {
    let mut addr = [0u8; HASHED_PUBKEY_SERIALIZED_SIZE];
    addr.copy_from_slice(&sha3::Keccak256::digest(&pubkey.serialize()[1..])[12..]);
    assert_eq!(addr.len(), HASHED_PUBKEY_SERIALIZED_SIZE);
    addr
}

pub fn verify(
    data: &[u8],
    instruction_datas: &[&[u8]],
    feature_set: &Arc<FeatureSet>,
) -> Result<(), PrecompileError> {
    if data.is_empty() {
        return Err(PrecompileError::InvalidInstructionDataSize);
    }
    let count = data[0] as usize;
    if (feature_set.is_active(&libsecp256k1_fail_on_bad_count::id())
        || feature_set.is_active(&libsecp256k1_fail_on_bad_count2::id()))
        && count == 0
        && data.len() > 1
    {
        // count is zero but the instruction data indicates that is probably not
        // correct, fail the instruction to catch probable invalid secp256k1
        // instruction construction.
        return Err(PrecompileError::InvalidInstructionDataSize);
    }
    let expected_data_size = count
        .saturating_mul(SIGNATURE_OFFSETS_SERIALIZED_SIZE)
        .saturating_add(1);
    if data.len() < expected_data_size {
        return Err(PrecompileError::InvalidInstructionDataSize);
    }
    for i in 0..count {
        let start = i
            .saturating_mul(SIGNATURE_OFFSETS_SERIALIZED_SIZE)
            .saturating_add(1);
        let end = start.saturating_add(SIGNATURE_OFFSETS_SERIALIZED_SIZE);

        let offsets: SecpSignatureOffsets = bincode::deserialize(&data[start..end])
            .map_err(|_| PrecompileError::InvalidSignature)?;

        // Parse out signature
        let signature_index = offsets.signature_instruction_index as usize;
        if signature_index >= instruction_datas.len() {
            return Err(PrecompileError::InvalidInstructionDataSize);
        }
        let signature_instruction = instruction_datas[signature_index];
        let sig_start = offsets.signature_offset as usize;
        let sig_end = sig_start.saturating_add(SIGNATURE_SERIALIZED_SIZE);
        if sig_end >= signature_instruction.len() {
            return Err(PrecompileError::InvalidSignature);
        }

        let sig_parse_result = if feature_set.is_active(&libsecp256k1_0_5_upgrade_enabled::id()) {
            libsecp256k1::Signature::parse_standard_slice(
                &signature_instruction[sig_start..sig_end],
            )
        } else {
            libsecp256k1::Signature::parse_overflowing_slice(
                &signature_instruction[sig_start..sig_end],
            )
        };

        let signature = sig_parse_result.map_err(|_| PrecompileError::InvalidSignature)?;

        let recovery_id = libsecp256k1::RecoveryId::parse(signature_instruction[sig_end])
            .map_err(|_| PrecompileError::InvalidRecoveryId)?;

        // Parse out pubkey
        let eth_address_slice = get_data_slice(
            instruction_datas,
            offsets.eth_address_instruction_index,
            offsets.eth_address_offset,
            HASHED_PUBKEY_SERIALIZED_SIZE,
        )?;

        // Parse out message
        let message_slice = get_data_slice(
            instruction_datas,
            offsets.message_instruction_index,
            offsets.message_data_offset,
            offsets.message_data_size as usize,
        )?;

        let mut hasher = sha3::Keccak256::new();
        hasher.update(message_slice);
        let message_hash = hasher.finalize();

        let pubkey = libsecp256k1::recover(
            &libsecp256k1::Message::parse_slice(&message_hash).unwrap(),
            &signature,
            &recovery_id,
        )
        .map_err(|_| PrecompileError::InvalidSignature)?;
        let eth_address = construct_eth_pubkey(&pubkey);

        if eth_address_slice != eth_address {
            return Err(PrecompileError::InvalidSignature);
        }
    }
    Ok(())
}

fn get_data_slice<'a>(
    instruction_datas: &'a [&[u8]],
    instruction_index: u8,
    offset_start: u16,
    size: usize,
) -> Result<&'a [u8], PrecompileError> {
    let signature_index = instruction_index as usize;
    if signature_index >= instruction_datas.len() {
        return Err(PrecompileError::InvalidDataOffsets);
    }
    let signature_instruction = &instruction_datas[signature_index];
    let start = offset_start as usize;
    let end = start.saturating_add(size);
    if end > signature_instruction.len() {
        return Err(PrecompileError::InvalidSignature);
    }

    Ok(&instruction_datas[signature_index][start..end])
}

#[cfg(test)]
pub mod test {
    use {
        super::*,
        crate::{
            feature_set,
            hash::Hash,
            secp256k1_instruction::{
                new_secp256k1_instruction, SecpSignatureOffsets, SIGNATURE_OFFSETS_SERIALIZED_SIZE,
            },
            signature::{Keypair, Signer},
            transaction::Transaction,
        },
        rand::{thread_rng, Rng},
        std::sync::Arc,
    };

    fn test_case(
        num_signatures: u8,
        offsets: &SecpSignatureOffsets,
    ) -> Result<(), PrecompileError> {
        let mut instruction_data = vec![0u8; DATA_START];
        instruction_data[0] = num_signatures;
        let writer = std::io::Cursor::new(&mut instruction_data[1..]);
        bincode::serialize_into(writer, &offsets).unwrap();
        let mut feature_set = FeatureSet::all_enabled();
        feature_set
            .active
            .remove(&libsecp256k1_0_5_upgrade_enabled::id());
        feature_set
            .inactive
            .insert(libsecp256k1_0_5_upgrade_enabled::id());

        verify(&instruction_data, &[&[0u8; 100]], &Arc::new(feature_set))
    }

    #[test]
    fn test_invalid_offsets() {
        solana_logger::setup();

        let mut instruction_data = vec![0u8; DATA_START];
        let offsets = SecpSignatureOffsets::default();
        instruction_data[0] = 1;
        let writer = std::io::Cursor::new(&mut instruction_data[1..]);
        bincode::serialize_into(writer, &offsets).unwrap();
        instruction_data.truncate(instruction_data.len() - 1);
        let mut feature_set = FeatureSet::all_enabled();
        feature_set
            .active
            .remove(&libsecp256k1_0_5_upgrade_enabled::id());
        feature_set
            .inactive
            .insert(libsecp256k1_0_5_upgrade_enabled::id());

        assert_eq!(
            verify(&instruction_data, &[&[0u8; 100]], &Arc::new(feature_set)),
            Err(PrecompileError::InvalidInstructionDataSize)
        );

        let offsets = SecpSignatureOffsets {
            signature_instruction_index: 1,
            ..SecpSignatureOffsets::default()
        };
        assert_eq!(
            test_case(1, &offsets),
            Err(PrecompileError::InvalidInstructionDataSize)
        );

        let offsets = SecpSignatureOffsets {
            message_instruction_index: 1,
            ..SecpSignatureOffsets::default()
        };
        assert_eq!(
            test_case(1, &offsets),
            Err(PrecompileError::InvalidDataOffsets)
        );

        let offsets = SecpSignatureOffsets {
            eth_address_instruction_index: 1,
            ..SecpSignatureOffsets::default()
        };
        assert_eq!(
            test_case(1, &offsets),
            Err(PrecompileError::InvalidDataOffsets)
        );
    }

    #[test]
    fn test_message_data_offsets() {
        let offsets = SecpSignatureOffsets {
            message_data_offset: 99,
            message_data_size: 1,
            ..SecpSignatureOffsets::default()
        };
        assert_eq!(
            test_case(1, &offsets),
            Err(PrecompileError::InvalidSignature)
        );

        let offsets = SecpSignatureOffsets {
            message_data_offset: 100,
            message_data_size: 1,
            ..SecpSignatureOffsets::default()
        };
        assert_eq!(
            test_case(1, &offsets),
            Err(PrecompileError::InvalidSignature)
        );

        let offsets = SecpSignatureOffsets {
            message_data_offset: 100,
            message_data_size: 1000,
            ..SecpSignatureOffsets::default()
        };
        assert_eq!(
            test_case(1, &offsets),
            Err(PrecompileError::InvalidSignature)
        );

        let offsets = SecpSignatureOffsets {
            message_data_offset: std::u16::MAX,
            message_data_size: std::u16::MAX,
            ..SecpSignatureOffsets::default()
        };
        assert_eq!(
            test_case(1, &offsets),
            Err(PrecompileError::InvalidSignature)
        );
    }

    #[test]
    fn test_eth_offset() {
        let offsets = SecpSignatureOffsets {
            eth_address_offset: std::u16::MAX,
            ..SecpSignatureOffsets::default()
        };
        assert_eq!(
            test_case(1, &offsets),
            Err(PrecompileError::InvalidSignature)
        );

        let offsets = SecpSignatureOffsets {
            eth_address_offset: 100 - HASHED_PUBKEY_SERIALIZED_SIZE as u16 + 1,
            ..SecpSignatureOffsets::default()
        };
        assert_eq!(
            test_case(1, &offsets),
            Err(PrecompileError::InvalidSignature)
        );
    }

    #[test]
    fn test_signature_offset() {
        let offsets = SecpSignatureOffsets {
            signature_offset: std::u16::MAX,
            ..SecpSignatureOffsets::default()
        };
        assert_eq!(
            test_case(1, &offsets),
            Err(PrecompileError::InvalidSignature)
        );

        let offsets = SecpSignatureOffsets {
            signature_offset: 100 - SIGNATURE_SERIALIZED_SIZE as u16 + 1,
            ..SecpSignatureOffsets::default()
        };
        assert_eq!(
            test_case(1, &offsets),
            Err(PrecompileError::InvalidSignature)
        );
    }

    #[test]
    fn test_count_is_zero_but_sig_data_exists() {
        solana_logger::setup();

        let mut instruction_data = vec![0u8; DATA_START];
        let offsets = SecpSignatureOffsets::default();
        instruction_data[0] = 0;
        let writer = std::io::Cursor::new(&mut instruction_data[1..]);
        bincode::serialize_into(writer, &offsets).unwrap();
        let mut feature_set = FeatureSet::all_enabled();
        feature_set
            .active
            .remove(&libsecp256k1_0_5_upgrade_enabled::id());
        feature_set
            .inactive
            .insert(libsecp256k1_0_5_upgrade_enabled::id());

        assert_eq!(
            verify(&instruction_data, &[&[0u8; 100]], &Arc::new(feature_set)),
            Err(PrecompileError::InvalidInstructionDataSize)
        );
    }

    #[test]
    fn test_secp256k1() {
        solana_logger::setup();
        let offsets = SecpSignatureOffsets::default();
        assert_eq!(
            bincode::serialized_size(&offsets).unwrap() as usize,
            SIGNATURE_OFFSETS_SERIALIZED_SIZE
        );

        let secp_privkey = libsecp256k1::SecretKey::random(&mut thread_rng());
        let message_arr = b"hello";
        let mut secp_instruction = new_secp256k1_instruction(&secp_privkey, message_arr);
        let mint_keypair = Keypair::new();
        let mut feature_set = feature_set::FeatureSet::all_enabled();
        feature_set
            .active
            .remove(&feature_set::libsecp256k1_0_5_upgrade_enabled::id());
        feature_set
            .inactive
            .insert(feature_set::libsecp256k1_0_5_upgrade_enabled::id());
        let feature_set = Arc::new(feature_set);

        let tx = Transaction::new_signed_with_payer(
            &[secp_instruction.clone()],
            Some(&mint_keypair.pubkey()),
            &[&mint_keypair],
            Hash::default(),
        );

        assert!(tx.verify_precompiles(&feature_set).is_ok());

        let index = thread_rng().gen_range(0, secp_instruction.data.len());
        secp_instruction.data[index] = secp_instruction.data[index].wrapping_add(12);
        let tx = Transaction::new_signed_with_payer(
            &[secp_instruction],
            Some(&mint_keypair.pubkey()),
            &[&mint_keypair],
            Hash::default(),
        );
        assert!(tx.verify_precompiles(&feature_set).is_err());
    }
}
