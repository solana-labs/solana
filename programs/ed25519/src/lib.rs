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
        ed25519_instruction::new_ed25519_instruction,
        feature_set::FeatureSet,
        hash::Hash,
        signature::{Keypair, Signer},
        transaction::Transaction,
    };
    use std::sync::Arc;

    #[test]
    fn test_ed25519() {
        solana_logger::setup();

        let privkey = ed25519_dalek::Keypair::generate(&mut thread_rng());
        let message_arr = b"hello";
        let mut instruction = new_ed25519_instruction(&privkey, message_arr);
        let mint_keypair = Keypair::new();
        let feature_set = Arc::new(FeatureSet::all_enabled());

        let tx = Transaction::new_signed_with_payer(
            &[instruction.clone()],
            Some(&mint_keypair.pubkey()),
            &[&mint_keypair],
            Hash::default(),
        );

        assert!(tx.verify_precompiles(&feature_set).is_ok());

        let index = thread_rng().gen_range(0, instruction.data.len());
        instruction.data[index] = instruction.data[index].wrapping_add(12);
        let tx = Transaction::new_signed_with_payer(
            &[instruction],
            Some(&mint_keypair.pubkey()),
            &[&mint_keypair],
            Hash::default(),
        );
        assert!(tx.verify_precompiles(&feature_set).is_err());
    }
}
