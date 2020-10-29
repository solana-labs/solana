use solana_sdk::pubkey::Pubkey;
use solana_sdk::{
    instruction::{Instruction, InstructionError},
    keyed_account::KeyedAccount,
};

pub fn process_instruction(
    _program_id: &Pubkey,
    _keyed_accounts: &[KeyedAccount],
    _data: &[u8],
) -> Result<(), InstructionError> {
    // Should be already checked by now.
    Ok(())
}

solana_sdk::declare_program!(
    solana_sdk::secp256k1_program::ID,
    solana_keccak_secp256k1_program,
    process_instruction
);

pub fn new_secp256k1_instruction(
    priv_key: &secp256k1::SecretKey,
    message_arr: &[u8],
) -> Instruction {
    use digest::Digest;
    use solana_sdk::secp256k1::{
        construct_eth_pubkey, SecpSignatureOffsets, SIGNATURE_OFFSETS_SERIALIZED_SIZE,
        SIGNATURE_SERIALIZED_SIZE,
    };

    let secp_pubkey = secp256k1::PublicKey::from_secret_key(priv_key);
    let eth_pubkey = construct_eth_pubkey(&secp_pubkey);
    let mut hasher = sha3::Keccak256::new();
    hasher.update(&message_arr);
    let message_hash = hasher.finalize();
    let mut message_hash_arr = [0u8; 32];
    message_hash_arr.copy_from_slice(&message_hash.as_slice());
    let message = secp256k1::Message::parse(&message_hash_arr);
    let (signature, recovery_id) = secp256k1::sign(&message, priv_key);
    let signature_arr = signature.serialize();
    assert_eq!(signature_arr.len(), SIGNATURE_SERIALIZED_SIZE);

    let mut instruction_data = vec![];
    let data_start = 1 + SIGNATURE_OFFSETS_SERIALIZED_SIZE;
    instruction_data.resize(
        data_start + eth_pubkey.len() + signature_arr.len() + message_arr.len() + 1,
        0,
    );
    let eth_address_offset = data_start;
    instruction_data[eth_address_offset..eth_address_offset + eth_pubkey.len()]
        .copy_from_slice(&eth_pubkey);

    let signature_offset = data_start + eth_pubkey.len();
    instruction_data[signature_offset..signature_offset + signature_arr.len()]
        .copy_from_slice(&signature_arr);

    instruction_data[signature_offset + signature_arr.len()] = recovery_id.serialize();

    let message_data_offset = signature_offset + signature_arr.len() + 1;
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
    let writer = std::io::Cursor::new(&mut instruction_data[1..data_start]);
    bincode::serialize_into(writer, &offsets).unwrap();

    Instruction {
        program_id: solana_sdk::secp256k1_program::id(),
        accounts: vec![],
        data: instruction_data,
    }
}

#[cfg(test)]
pub mod test {
    use rand::{thread_rng, Rng};
    use solana_sdk::secp256k1::{SecpSignatureOffsets, SIGNATURE_OFFSETS_SERIALIZED_SIZE};
    use solana_sdk::{
        hash::Hash,
        signature::{Keypair, Signer},
        transaction::Transaction,
    };

    #[test]
    fn test_secp256k1() {
        solana_logger::setup();
        let offsets = SecpSignatureOffsets::default();
        assert_eq!(
            bincode::serialized_size(&offsets).unwrap() as usize,
            SIGNATURE_OFFSETS_SERIALIZED_SIZE
        );

        let secp_privkey = secp256k1::SecretKey::random(&mut thread_rng());
        let message_arr = b"hello";
        let mut secp_instruction = super::new_secp256k1_instruction(&secp_privkey, message_arr);
        let mint_keypair = Keypair::new();

        let tx = Transaction::new_signed_with_payer(
            &[secp_instruction.clone()],
            Some(&mint_keypair.pubkey()),
            &[&mint_keypair],
            Hash::default(),
        );

        assert!(tx.verify_precompiles().is_ok());

        let index = thread_rng().gen_range(0, secp_instruction.data.len());
        secp_instruction.data[index] = secp_instruction.data[index].wrapping_add(12);
        let tx = Transaction::new_signed_with_payer(
            &[secp_instruction],
            Some(&mint_keypair.pubkey()),
            &[&mint_keypair],
            Hash::default(),
        );
        assert!(tx.verify_precompiles().is_err());
    }
}
