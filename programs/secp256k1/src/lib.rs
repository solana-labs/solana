use solana_sdk::{
    instruction::InstructionError, process_instruction::InvokeContext, pubkey::Pubkey,
};

pub fn process_instruction(
    _program_id: &Pubkey,
    _data: &[u8],
    _invoke_context: &mut dyn InvokeContext,
) -> Result<(), InstructionError> {
    // Should be already checked by now.
    Ok(())
}

#[cfg(test)]
pub mod test {
    use rand::{thread_rng, Rng};
    use solana_sdk::{
        hash::Hash,
        secp256k1_instruction::{
            new_secp256k1_instruction, SecpSignatureOffsets, SIGNATURE_OFFSETS_SERIALIZED_SIZE,
        },
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

        let secp_privkey = libsecp256k1::SecretKey::random(&mut thread_rng());
        let message_arr = b"hello";
        let mut secp_instruction = new_secp256k1_instruction(&secp_privkey, message_arr);
        let mint_keypair = Keypair::new();

        let tx = Transaction::new_signed_with_payer(
            &[secp_instruction.clone()],
            Some(&mint_keypair.pubkey()),
            &[&mint_keypair],
            Hash::default(),
        );

        assert!(tx.verify_precompiles(false, true).is_ok());

        let index = thread_rng().gen_range(0, secp_instruction.data.len());
        secp_instruction.data[index] = secp_instruction.data[index].wrapping_add(12);
        let tx = Transaction::new_signed_with_payer(
            &[secp_instruction],
            Some(&mint_keypair.pubkey()),
            &[&mint_keypair],
            Hash::default(),
        );
        assert!(tx.verify_precompiles(false, true).is_err());
    }
}
