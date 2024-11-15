//! Instructions for the [secp256r1 native program][np].
//! [np]: https://docs.solana.com/developing/runtime-facilities/programs#secp256r1-program
//!
//! Note on Signature Malleability:
//! This precompile requires low-S values in signatures (s <= half_curve_order) to prevent signature malleability.
//! Signature malleability means that for a valid signature (r,s), (r, order-s) is also valid for the
//! same message and public key.
//!
//! This property can be problematic for developers who assume each signature is unique. Without enforcing
//! low-S values, the same message and key can produce two different valid signatures, potentially breaking
//! replay protection schemes that rely on signature uniqueness.
solana_pubkey::declare_id!("Secp256r1SigVerify1111111111111111111111111");

use bytemuck::{Pod, Zeroable};

#[derive(Default, Debug, Copy, Clone, Zeroable, Pod, Eq, PartialEq)]
#[repr(C)]
pub struct Secp256r1SignatureOffsets {
    /// Offset to compact secp256r1 signature of 64 bytes
    pub signature_offset: u16,

    /// Instruction index where the signature can be found
    pub signature_instruction_index: u16,

    /// Offset to compressed public key of 33 bytes
    pub public_key_offset: u16,

    /// Instruction index where the public key can be found
    pub public_key_instruction_index: u16,

    /// Offset to the start of message data
    pub message_data_offset: u16,

    /// Size of message data in bytes
    pub message_data_size: u16,

    /// Instruction index where the message data can be found
    pub message_instruction_index: u16,
}

#[cfg(all(not(target_arch = "wasm32"), not(target_os = "solana")))]
mod target_arch {
    use {
        crate::Secp256r1SignatureOffsets,
        bytemuck::bytes_of,
        openssl::{
            bn::{BigNum, BigNumContext},
            ec::{EcGroup, EcKey, EcPoint},
            ecdsa::EcdsaSig,
            nid::Nid,
            pkey::{PKey, Private},
            sign::{Signer, Verifier},
        },
        solana_feature_set::FeatureSet,
        solana_instruction::Instruction,
        solana_precompile_error::PrecompileError,
    };

    pub const COMPRESSED_PUBKEY_SERIALIZED_SIZE: usize = 33;
    pub const SIGNATURE_SERIALIZED_SIZE: usize = 64;
    pub const SIGNATURE_OFFSETS_SERIALIZED_SIZE: usize = 14;
    pub const SIGNATURE_OFFSETS_START: usize = 2;
    pub const DATA_START: usize = SIGNATURE_OFFSETS_SERIALIZED_SIZE + SIGNATURE_OFFSETS_START;

    // Order as defined in SEC2: 2.7.2 Recommended Parameters secp256r1
    pub const SECP256R1_ORDER: [u8; FIELD_SIZE] = [
        0xFF, 0xFF, 0xFF, 0xFF, 0x00, 0x00, 0x00, 0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
        0xFF, 0xBC, 0xE6, 0xFA, 0xAD, 0xA7, 0x17, 0x9E, 0x84, 0xF3, 0xB9, 0xCA, 0xC2, 0xFC, 0x63,
        0x25, 0x51,
    ];

    // Computed SECP256R1_ORDER - 1
    pub const SECP256R1_ORDER_MINUS_ONE: [u8; FIELD_SIZE] = [
        0xFF, 0xFF, 0xFF, 0xFF, 0x00, 0x00, 0x00, 0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
        0xFF, 0xBC, 0xE6, 0xFA, 0xAD, 0xA7, 0x17, 0x9E, 0x84, 0xF3, 0xB9, 0xCA, 0xC2, 0xFC, 0x63,
        0x25, 0x50,
    ];

    // Computed half order
    const SECP256R1_HALF_ORDER: [u8; FIELD_SIZE] = [
        0x7F, 0xFF, 0xFF, 0xFF, 0x80, 0x00, 0x00, 0x00, 0x7F, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
        0xFF, 0xDE, 0x73, 0x7D, 0x56, 0xD3, 0x8B, 0xCF, 0x42, 0x79, 0xDC, 0xE5, 0x61, 0x7E, 0x31,
        0x92, 0xA8,
    ];
    // Field size in bytes
    const FIELD_SIZE: usize = 32;

    pub fn new_secp256r1_instruction(
        message: &[u8],
        signing_key: EcKey<Private>,
    ) -> Result<Instruction, Box<dyn std::error::Error>> {
        let group = EcGroup::from_curve_name(Nid::X9_62_PRIME256V1)?;
        if signing_key.group().curve_name() != Some(Nid::X9_62_PRIME256V1) {
            return Err(("Signing key must be on the secp256r1 curve".to_string()).into());
        }

        let mut ctx = BigNumContext::new()?;
        let pubkey = signing_key.public_key().to_bytes(
            &group,
            openssl::ec::PointConversionForm::COMPRESSED,
            &mut ctx,
        )?;

        let signing_key_pkey = PKey::from_ec_key(signing_key)?;

        let mut signer = Signer::new(openssl::hash::MessageDigest::sha256(), &signing_key_pkey)?;
        signer.update(message)?;
        let signature = signer.sign_to_vec()?;

        let ecdsa_sig = EcdsaSig::from_der(&signature)?;
        let r = ecdsa_sig.r().to_vec();
        let s = ecdsa_sig.s().to_vec();
        let mut signature = vec![0u8; SIGNATURE_SERIALIZED_SIZE];

        // Incase of an r or s value of 31 bytes we need to pad it to 32 bytes
        let mut padded_r = vec![0u8; FIELD_SIZE];
        let mut padded_s = vec![0u8; FIELD_SIZE];
        padded_r[FIELD_SIZE.saturating_sub(r.len())..].copy_from_slice(&r);
        padded_s[FIELD_SIZE.saturating_sub(s.len())..].copy_from_slice(&s);

        signature[..FIELD_SIZE].copy_from_slice(&padded_r);
        signature[FIELD_SIZE..].copy_from_slice(&padded_s);

        // Check if s > half_order, if so, compute s = order - s
        let s_bignum = BigNum::from_slice(&s)?;
        let half_order = BigNum::from_slice(&SECP256R1_HALF_ORDER)?;
        let order = BigNum::from_slice(&SECP256R1_ORDER)?;
        if s_bignum > half_order {
            let mut new_s = BigNum::new()?;
            new_s.checked_sub(&order, &s_bignum)?;
            let new_s_bytes = new_s.to_vec();

            // Incase the new s value is 31 bytes we need to pad it to 32 bytes
            let mut new_padded_s = vec![0u8; FIELD_SIZE];
            new_padded_s[FIELD_SIZE.saturating_sub(new_s_bytes.len())..]
                .copy_from_slice(&new_s_bytes);

            signature[FIELD_SIZE..].copy_from_slice(&new_padded_s);
        }

        assert_eq!(pubkey.len(), COMPRESSED_PUBKEY_SERIALIZED_SIZE);
        assert_eq!(signature.len(), SIGNATURE_SERIALIZED_SIZE);

        let mut instruction_data = Vec::with_capacity(
            DATA_START
                .saturating_add(SIGNATURE_SERIALIZED_SIZE)
                .saturating_add(COMPRESSED_PUBKEY_SERIALIZED_SIZE)
                .saturating_add(message.len()),
        );

        let num_signatures: u8 = 1;
        let public_key_offset = DATA_START;
        let signature_offset = public_key_offset.saturating_add(COMPRESSED_PUBKEY_SERIALIZED_SIZE);
        let message_data_offset = signature_offset.saturating_add(SIGNATURE_SERIALIZED_SIZE);

        instruction_data.extend_from_slice(bytes_of(&[num_signatures, 0]));

        let offsets = Secp256r1SignatureOffsets {
            signature_offset: signature_offset as u16,
            signature_instruction_index: u16::MAX,
            public_key_offset: public_key_offset as u16,
            public_key_instruction_index: u16::MAX,
            message_data_offset: message_data_offset as u16,
            message_data_size: message.len() as u16,
            message_instruction_index: u16::MAX,
        };

        instruction_data.extend_from_slice(bytes_of(&offsets));
        instruction_data.extend_from_slice(&pubkey);
        instruction_data.extend_from_slice(&signature);
        instruction_data.extend_from_slice(message);

        Ok(Instruction {
            program_id: crate::id(),
            accounts: vec![],
            data: instruction_data,
        })
    }

    pub fn verify(
        data: &[u8],
        instruction_datas: &[&[u8]],
        _feature_set: &FeatureSet,
    ) -> Result<(), PrecompileError> {
        if data.len() < SIGNATURE_OFFSETS_START {
            return Err(PrecompileError::InvalidInstructionDataSize);
        }
        let num_signatures = data[0] as usize;
        if num_signatures == 0 {
            return Err(PrecompileError::InvalidInstructionDataSize);
        }
        if num_signatures > 8 {
            return Err(PrecompileError::InvalidInstructionDataSize);
        }

        let expected_data_size = num_signatures
            .saturating_mul(SIGNATURE_OFFSETS_SERIALIZED_SIZE)
            .saturating_add(SIGNATURE_OFFSETS_START);

        // We do not check or use the byte at data[1]
        if data.len() < expected_data_size {
            return Err(PrecompileError::InvalidInstructionDataSize);
        }

        // Parse half order from constant
        let half_order: BigNum = BigNum::from_slice(&SECP256R1_HALF_ORDER)
            .map_err(|_| PrecompileError::InvalidSignature)?;

        // Parse order - 1 from constant
        let order_minus_one: BigNum = BigNum::from_slice(&SECP256R1_ORDER_MINUS_ONE)
            .map_err(|_| PrecompileError::InvalidSignature)?;

        // Create a BigNum for 1
        let one = BigNum::from_u32(1).map_err(|_| PrecompileError::InvalidSignature)?;

        // Define curve group
        let group = EcGroup::from_curve_name(Nid::X9_62_PRIME256V1)
            .map_err(|_| PrecompileError::InvalidSignature)?;
        let mut ctx = BigNumContext::new().map_err(|_| PrecompileError::InvalidSignature)?;

        for i in 0..num_signatures {
            let start = i
                .saturating_mul(SIGNATURE_OFFSETS_SERIALIZED_SIZE)
                .saturating_add(SIGNATURE_OFFSETS_START);
            let end = start.saturating_add(SIGNATURE_OFFSETS_SERIALIZED_SIZE);

            // bytemuck wants structures aligned
            let offsets: &Secp256r1SignatureOffsets =
                bytemuck::try_from_bytes(&data[start..end])
                    .map_err(|_| PrecompileError::InvalidDataOffsets)?;

            // Parse out signature
            let signature = get_data_slice(
                data,
                instruction_datas,
                offsets.signature_instruction_index,
                offsets.signature_offset,
                SIGNATURE_SERIALIZED_SIZE,
            )?;

            // Parse out pubkey
            let pubkey = get_data_slice(
                data,
                instruction_datas,
                offsets.public_key_instruction_index,
                offsets.public_key_offset,
                COMPRESSED_PUBKEY_SERIALIZED_SIZE,
            )?;

            // Parse out message
            let message = get_data_slice(
                data,
                instruction_datas,
                offsets.message_instruction_index,
                offsets.message_data_offset,
                offsets.message_data_size as usize,
            )?;

            let r_bignum = BigNum::from_slice(&signature[..FIELD_SIZE])
                .map_err(|_| PrecompileError::InvalidSignature)?;
            let s_bignum = BigNum::from_slice(&signature[FIELD_SIZE..])
                .map_err(|_| PrecompileError::InvalidSignature)?;

            // Check that the signature is generally in range
            let within_range = r_bignum >= one
                && r_bignum <= order_minus_one
                && s_bignum >= one
                && s_bignum <= half_order;

            if !within_range {
                return Err(PrecompileError::InvalidSignature);
            }

            // Create an ECDSA signature object from the ASN.1 integers
            let ecdsa_sig = openssl::ecdsa::EcdsaSig::from_private_components(r_bignum, s_bignum)
                .and_then(|sig| sig.to_der())
                .map_err(|_| PrecompileError::InvalidSignature)?;

            let public_key_point = EcPoint::from_bytes(&group, pubkey, &mut ctx)
                .map_err(|_| PrecompileError::InvalidPublicKey)?;
            let public_key = EcKey::from_public_key(&group, &public_key_point)
                .map_err(|_| PrecompileError::InvalidPublicKey)?;
            let public_key_as_pkey =
                PKey::from_ec_key(public_key).map_err(|_| PrecompileError::InvalidPublicKey)?;

            let mut verifier =
                Verifier::new(openssl::hash::MessageDigest::sha256(), &public_key_as_pkey)
                    .map_err(|_| PrecompileError::InvalidSignature)?;
            verifier
                .update(message)
                .map_err(|_| PrecompileError::InvalidSignature)?;

            if !verifier
                .verify(&ecdsa_sig)
                .map_err(|_| PrecompileError::InvalidSignature)?
            {
                return Err(PrecompileError::InvalidSignature);
            }
        }
        Ok(())
    }

    fn get_data_slice<'a>(
        data: &'a [u8],
        instruction_datas: &'a [&[u8]],
        instruction_index: u16,
        offset_start: u16,
        size: usize,
    ) -> Result<&'a [u8], PrecompileError> {
        let instruction = if instruction_index == u16::MAX {
            data
        } else {
            let signature_index = instruction_index as usize;
            if signature_index >= instruction_datas.len() {
                return Err(PrecompileError::InvalidDataOffsets);
            }
            instruction_datas[signature_index]
        };

        let start = offset_start as usize;
        let end = start.saturating_add(size);
        if end > instruction.len() {
            return Err(PrecompileError::InvalidDataOffsets);
        }

        Ok(&instruction[start..end])
    }

    #[cfg(test)]
    mod test {
        use {
            super::*,
            solana_feature_set::FeatureSet,
            solana_sdk::{
                hash::Hash,
                signature::{Keypair, Signer},
                transaction::Transaction,
            },
        };

        fn test_case(
            num_signatures: u16,
            offsets: &Secp256r1SignatureOffsets,
        ) -> Result<(), PrecompileError> {
            assert_eq!(
                bytemuck::bytes_of(offsets).len(),
                SIGNATURE_OFFSETS_SERIALIZED_SIZE
            );

            let mut instruction_data = vec![0u8; DATA_START];
            instruction_data[0..SIGNATURE_OFFSETS_START].copy_from_slice(bytes_of(&num_signatures));
            instruction_data[SIGNATURE_OFFSETS_START..DATA_START]
                .copy_from_slice(bytes_of(offsets));
            verify(
                &instruction_data,
                &[&[0u8; 100]],
                &FeatureSet::all_enabled(),
            )
        }

        #[test]
        fn test_invalid_offsets() {
            solana_logger::setup();

            let mut instruction_data = vec![0u8; DATA_START];
            let offsets = Secp256r1SignatureOffsets::default();
            instruction_data[0..SIGNATURE_OFFSETS_START].copy_from_slice(bytes_of(&1u16));
            instruction_data[SIGNATURE_OFFSETS_START..DATA_START]
                .copy_from_slice(bytes_of(&offsets));
            instruction_data.truncate(instruction_data.len() - 1);

            assert_eq!(
                verify(
                    &instruction_data,
                    &[&[0u8; 100]],
                    &FeatureSet::all_enabled()
                ),
                Err(PrecompileError::InvalidInstructionDataSize)
            );

            let offsets = Secp256r1SignatureOffsets {
                signature_instruction_index: 1,
                ..Secp256r1SignatureOffsets::default()
            };
            assert_eq!(
                test_case(1, &offsets),
                Err(PrecompileError::InvalidDataOffsets)
            );

            let offsets = Secp256r1SignatureOffsets {
                message_instruction_index: 1,
                ..Secp256r1SignatureOffsets::default()
            };
            assert_eq!(
                test_case(1, &offsets),
                Err(PrecompileError::InvalidDataOffsets)
            );

            let offsets = Secp256r1SignatureOffsets {
                public_key_instruction_index: 1,
                ..Secp256r1SignatureOffsets::default()
            };
            assert_eq!(
                test_case(1, &offsets),
                Err(PrecompileError::InvalidDataOffsets)
            );
        }

        #[test]
        fn test_invalid_signature_data_size() {
            solana_logger::setup();

            // Test data.len() < SIGNATURE_OFFSETS_START
            let small_data = vec![0u8; SIGNATURE_OFFSETS_START - 1];
            assert_eq!(
                verify(&small_data, &[&[]], &FeatureSet::all_enabled()),
                Err(PrecompileError::InvalidInstructionDataSize)
            );

            // Test num_signatures == 0
            let mut zero_sigs_data = vec![0u8; DATA_START];
            zero_sigs_data[0] = 0; // Set num_signatures to 0
            assert_eq!(
                verify(&zero_sigs_data, &[&[]], &FeatureSet::all_enabled()),
                Err(PrecompileError::InvalidInstructionDataSize)
            );

            // Test num_signatures > 8
            let mut too_many_sigs = vec![0u8; DATA_START];
            too_many_sigs[0] = 9; // Set num_signatures to 9
            assert_eq!(
                verify(&too_many_sigs, &[&[]], &FeatureSet::all_enabled()),
                Err(PrecompileError::InvalidInstructionDataSize)
            );
        }
        #[test]
        fn test_message_data_offsets() {
            let offsets = Secp256r1SignatureOffsets {
                message_data_offset: 99,
                message_data_size: 1,
                ..Secp256r1SignatureOffsets::default()
            };
            assert_eq!(
                test_case(1, &offsets),
                Err(PrecompileError::InvalidSignature)
            );

            let offsets = Secp256r1SignatureOffsets {
                message_data_offset: 100,
                message_data_size: 1,
                ..Secp256r1SignatureOffsets::default()
            };
            assert_eq!(
                test_case(1, &offsets),
                Err(PrecompileError::InvalidDataOffsets)
            );

            let offsets = Secp256r1SignatureOffsets {
                message_data_offset: 100,
                message_data_size: 1000,
                ..Secp256r1SignatureOffsets::default()
            };
            assert_eq!(
                test_case(1, &offsets),
                Err(PrecompileError::InvalidDataOffsets)
            );

            let offsets = Secp256r1SignatureOffsets {
                message_data_offset: u16::MAX,
                message_data_size: u16::MAX,
                ..Secp256r1SignatureOffsets::default()
            };
            assert_eq!(
                test_case(1, &offsets),
                Err(PrecompileError::InvalidDataOffsets)
            );
        }

        #[test]
        fn test_pubkey_offset() {
            let offsets = Secp256r1SignatureOffsets {
                public_key_offset: u16::MAX,
                ..Secp256r1SignatureOffsets::default()
            };
            assert_eq!(
                test_case(1, &offsets),
                Err(PrecompileError::InvalidDataOffsets)
            );

            let offsets = Secp256r1SignatureOffsets {
                public_key_offset: 100 - (COMPRESSED_PUBKEY_SERIALIZED_SIZE as u16) + 1,
                ..Secp256r1SignatureOffsets::default()
            };
            assert_eq!(
                test_case(1, &offsets),
                Err(PrecompileError::InvalidDataOffsets)
            );
        }

        #[test]
        fn test_signature_offset() {
            let offsets = Secp256r1SignatureOffsets {
                signature_offset: u16::MAX,
                ..Secp256r1SignatureOffsets::default()
            };
            assert_eq!(
                test_case(1, &offsets),
                Err(PrecompileError::InvalidDataOffsets)
            );

            let offsets = Secp256r1SignatureOffsets {
                signature_offset: 100 - (SIGNATURE_SERIALIZED_SIZE as u16) + 1,
                ..Secp256r1SignatureOffsets::default()
            };
            assert_eq!(
                test_case(1, &offsets),
                Err(PrecompileError::InvalidDataOffsets)
            );
        }

        #[test]
        fn test_secp256r1() {
            solana_logger::setup();
            let message_arr = b"hello";
            let group = EcGroup::from_curve_name(Nid::X9_62_PRIME256V1).unwrap();
            let signing_key = EcKey::generate(&group).unwrap();
            let mut instruction = new_secp256r1_instruction(message_arr, signing_key).unwrap();
            let mint_keypair = Keypair::new();
            let feature_set = FeatureSet::all_enabled();

            let tx = Transaction::new_signed_with_payer(
                &[instruction.clone()],
                Some(&mint_keypair.pubkey()),
                &[&mint_keypair],
                Hash::default(),
            );

            assert!(tx.verify_precompiles(&feature_set).is_ok());

            // The message is the last field in the instruction data so
            // changing its last byte will also change the signature validity
            let message_byte_index = instruction.data.len() - 1;
            instruction.data[message_byte_index] =
                instruction.data[message_byte_index].wrapping_add(12);
            let tx = Transaction::new_signed_with_payer(
                &[instruction.clone()],
                Some(&mint_keypair.pubkey()),
                &[&mint_keypair],
                Hash::default(),
            );

            assert!(tx.verify_precompiles(&feature_set).is_err());
        }

        #[test]
        fn test_secp256r1_high_s() {
            solana_logger::setup();
            let message_arr = b"hello";
            let group = EcGroup::from_curve_name(Nid::X9_62_PRIME256V1).unwrap();
            let signing_key = EcKey::generate(&group).unwrap();
            let mut instruction = new_secp256r1_instruction(message_arr, signing_key).unwrap();

            // To double check that the untampered low-S value signature passes
            let feature_set = FeatureSet::all_enabled();
            let tx_pass = verify(
                instruction.data.as_slice(),
                &[instruction.data.as_slice()],
                &feature_set,
            );
            assert!(tx_pass.is_ok());

            // Determine offsets at which to perform the S-value manipulation
            let public_key_offset = DATA_START;
            let signature_offset = public_key_offset + COMPRESSED_PUBKEY_SERIALIZED_SIZE;
            let s_offset = signature_offset + FIELD_SIZE;

            // Create a high S value by doing order - s
            let order = BigNum::from_slice(&SECP256R1_ORDER).unwrap();
            let current_s =
                BigNum::from_slice(&instruction.data[s_offset..s_offset + FIELD_SIZE]).unwrap();
            let mut high_s = BigNum::new().unwrap();
            high_s.checked_sub(&order, &current_s).unwrap();

            // Replace the S value in the signature with our high S
            instruction.data[s_offset..s_offset + FIELD_SIZE].copy_from_slice(&high_s.to_vec());

            // Since Transaction::verify_precompiles only returns a vague
            // `InvalidAccountIndex` error on precompile failure, we use verify()
            // here direclty to check for the specific
            // InvalidSignatureValueRange error
            let tx_fail = verify(
                instruction.data.as_slice(),
                &[instruction.data.as_slice()],
                &feature_set,
            );
            assert!(tx_fail.unwrap_err() == PrecompileError::InvalidSignature);
        }
        #[test]
        fn test_new_secp256r1_instruction_31byte_components() {
            solana_logger::setup();
            let message_arr = b"hello";
            let group = EcGroup::from_curve_name(Nid::X9_62_PRIME256V1).unwrap();
            let signing_key = EcKey::generate(&group).unwrap();

            // Keep generating signatures until we get one with a 31-byte component
            loop {
                let instruction =
                    new_secp256r1_instruction(message_arr, signing_key.clone()).unwrap();

                // Extract r and s from the signature
                let signature_offset = DATA_START + COMPRESSED_PUBKEY_SERIALIZED_SIZE;
                let r = &instruction.data[signature_offset..signature_offset + FIELD_SIZE];
                let s = &instruction.data
                    [signature_offset + FIELD_SIZE..signature_offset + 2 * FIELD_SIZE];

                // Convert to BigNum and back to get byte representation
                let r_bn = BigNum::from_slice(r).unwrap();
                let s_bn = BigNum::from_slice(s).unwrap();
                let r_bytes = r_bn.to_vec();
                let s_bytes = s_bn.to_vec();

                if r_bytes.len() == 31 || s_bytes.len() == 31 {
                    // Once found, verify the signature and break out of the loop
                    let mint_keypair = Keypair::new();
                    let tx = Transaction::new_signed_with_payer(
                        &[instruction],
                        Some(&mint_keypair.pubkey()),
                        &[&mint_keypair],
                        Hash::default(),
                    );

                    let feature_set = FeatureSet::all_enabled();
                    assert!(tx.verify_precompiles(&feature_set).is_ok());
                    break;
                }
            }
        }

        #[test]
        fn test_new_secp256r1_instruction_signing_key() {
            solana_logger::setup();
            let message_arr = b"hello";
            let group = EcGroup::from_curve_name(Nid::X9_62_PRIME256V1).unwrap();
            let signing_key = EcKey::generate(&group).unwrap();
            assert!(new_secp256r1_instruction(message_arr, signing_key).is_ok());

            let incorrect_group = EcGroup::from_curve_name(Nid::X9_62_PRIME192V1).unwrap();
            let incorrect_key = EcKey::generate(&incorrect_group).unwrap();
            assert!(new_secp256r1_instruction(message_arr, incorrect_key).is_err());
        }
        #[test]
        fn test_secp256r1_order() {
            let group = EcGroup::from_curve_name(Nid::X9_62_PRIME256V1).unwrap();
            let mut ctx = BigNumContext::new().unwrap();
            let mut openssl_order = BigNum::new().unwrap();
            group.order(&mut openssl_order, &mut ctx).unwrap();

            let our_order = BigNum::from_slice(&SECP256R1_ORDER).unwrap();
            assert_eq!(our_order, openssl_order);
        }

        #[test]
        fn test_secp256r1_order_minus_one() {
            let group = EcGroup::from_curve_name(Nid::X9_62_PRIME256V1).unwrap();
            let mut ctx = BigNumContext::new().unwrap();
            let mut openssl_order = BigNum::new().unwrap();
            group.order(&mut openssl_order, &mut ctx).unwrap();

            let mut expected_order_minus_one = BigNum::new().unwrap();
            expected_order_minus_one
                .checked_sub(&openssl_order, &BigNum::from_u32(1).unwrap())
                .unwrap();

            let our_order_minus_one = BigNum::from_slice(&SECP256R1_ORDER_MINUS_ONE).unwrap();
            assert_eq!(our_order_minus_one, expected_order_minus_one);
        }

        #[test]
        fn test_secp256r1_half_order() {
            // Get the secp256r1 curve group
            let group = EcGroup::from_curve_name(Nid::X9_62_PRIME256V1).unwrap();

            // Get the order from OpenSSL
            let mut ctx = BigNumContext::new().unwrap();
            let mut openssl_order = BigNum::new().unwrap();
            group.order(&mut openssl_order, &mut ctx).unwrap();

            // Calculate half order
            let mut calculated_half_order = BigNum::new().unwrap();
            let two = BigNum::from_u32(2).unwrap();
            calculated_half_order
                .checked_div(&openssl_order, &two, &mut ctx)
                .unwrap();

            // Get our constant half order
            let our_half_order = BigNum::from_slice(&SECP256R1_HALF_ORDER).unwrap();

            // Compare the calculated half order with our constant
            assert_eq!(calculated_half_order, our_half_order);
        }

        #[test]
        fn test_secp256r1_order_relationships() {
            let group = EcGroup::from_curve_name(Nid::X9_62_PRIME256V1).unwrap();
            let mut ctx = BigNumContext::new().unwrap();
            let mut openssl_order = BigNum::new().unwrap();
            group.order(&mut openssl_order, &mut ctx).unwrap();

            let our_order = BigNum::from_slice(&SECP256R1_ORDER).unwrap();
            let our_order_minus_one = BigNum::from_slice(&SECP256R1_ORDER_MINUS_ONE).unwrap();
            let our_half_order = BigNum::from_slice(&SECP256R1_HALF_ORDER).unwrap();

            // Verify our order matches OpenSSL's order
            assert_eq!(our_order, openssl_order);

            // Verify order - 1
            let mut expected_order_minus_one = BigNum::new().unwrap();
            expected_order_minus_one
                .checked_sub(&openssl_order, &BigNum::from_u32(1).unwrap())
                .unwrap();
            assert_eq!(our_order_minus_one, expected_order_minus_one);

            // Verify half order
            let mut expected_half_order = BigNum::new().unwrap();
            expected_half_order
                .checked_div(&openssl_order, &BigNum::from_u32(2).unwrap(), &mut ctx)
                .unwrap();
            assert_eq!(our_half_order, expected_half_order);

            // Verify half order * 2 = order - 1
            let mut double_half_order = BigNum::new().unwrap();
            double_half_order
                .checked_mul(&our_half_order, &BigNum::from_u32(2).unwrap(), &mut ctx)
                .unwrap();
            assert_eq!(double_half_order, expected_order_minus_one);
        }
    }
}

#[cfg(any(target_arch = "wasm32", target_os = "solana"))]
mod target_arch {
    use {solana_feature_set::FeatureSet, solana_precompile_error::PrecompileError};

    pub fn verify(
        _data: &[u8],
        _instruction_datas: &[&[u8]],
        _feature_set: &FeatureSet,
    ) -> Result<(), PrecompileError> {
        Err(PrecompileError::InvalidSignature)
    }
}

pub use self::target_arch::*;
