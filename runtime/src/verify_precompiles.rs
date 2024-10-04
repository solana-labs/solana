use {
    solana_feature_set::FeatureSet,
    solana_sdk::{
        instruction::InstructionError,
        precompiles::get_precompiles,
        transaction::{Result, TransactionError},
    },
    solana_svm_transaction::svm_message::SVMMessage,
};

pub fn verify_precompiles(message: &impl SVMMessage, feature_set: &FeatureSet) -> Result<()> {
    let mut all_instruction_data = None; // lazily collect this on first pre-compile

    let precompiles = get_precompiles();
    for (index, (program_id, instruction)) in message.program_instructions_iter().enumerate() {
        for precompile in precompiles {
            if precompile.check_id(program_id, |id| feature_set.is_active(id)) {
                let all_instruction_data: &Vec<&[u8]> = all_instruction_data
                    .get_or_insert_with(|| message.instructions_iter().map(|ix| ix.data).collect());
                precompile
                    .verify(instruction.data, all_instruction_data, feature_set)
                    .map_err(|err| {
                        TransactionError::InstructionError(
                            index as u8,
                            InstructionError::Custom(err as u32),
                        )
                    })?;
                break;
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        rand0_7::{thread_rng, Rng},
        solana_sdk::{
            ed25519_instruction::new_ed25519_instruction,
            hash::Hash,
            pubkey::Pubkey,
            secp256k1_instruction::new_secp256k1_instruction,
            signature::Keypair,
            signer::Signer,
            system_instruction, system_transaction,
            transaction::{SanitizedTransaction, Transaction},
        },
    };

    #[test]
    fn test_verify_precompiles_simple_transaction() {
        let tx = SanitizedTransaction::from_transaction_for_tests(system_transaction::transfer(
            &Keypair::new(),
            &Pubkey::new_unique(),
            1,
            Hash::default(),
        ));
        assert!(verify_precompiles(&tx, &FeatureSet::all_enabled()).is_ok());
    }

    #[test]
    fn test_verify_precompiles_secp256k1() {
        let secp_privkey = libsecp256k1::SecretKey::random(&mut thread_rng());
        let message_arr = b"hello";
        let mut secp_instruction = new_secp256k1_instruction(&secp_privkey, message_arr);
        let mint_keypair = Keypair::new();
        let feature_set = FeatureSet::all_enabled();

        let tx =
            SanitizedTransaction::from_transaction_for_tests(Transaction::new_signed_with_payer(
                &[secp_instruction.clone()],
                Some(&mint_keypair.pubkey()),
                &[&mint_keypair],
                Hash::default(),
            ));

        assert!(verify_precompiles(&tx, &feature_set).is_ok());

        let index = thread_rng().gen_range(0, secp_instruction.data.len());
        secp_instruction.data[index] = secp_instruction.data[index].wrapping_add(12);
        let tx =
            SanitizedTransaction::from_transaction_for_tests(Transaction::new_signed_with_payer(
                &[secp_instruction],
                Some(&mint_keypair.pubkey()),
                &[&mint_keypair],
                Hash::default(),
            ));

        assert!(verify_precompiles(&tx, &feature_set).is_err());
    }

    #[test]
    fn test_verify_precompiles_ed25519() {
        let privkey = ed25519_dalek::Keypair::generate(&mut thread_rng());
        let message_arr = b"hello";
        let mut instruction = new_ed25519_instruction(&privkey, message_arr);
        let mint_keypair = Keypair::new();
        let feature_set = FeatureSet::all_enabled();

        let tx =
            SanitizedTransaction::from_transaction_for_tests(Transaction::new_signed_with_payer(
                &[instruction.clone()],
                Some(&mint_keypair.pubkey()),
                &[&mint_keypair],
                Hash::default(),
            ));

        assert!(verify_precompiles(&tx, &feature_set).is_ok());

        let index = loop {
            let index = thread_rng().gen_range(0, instruction.data.len());
            // byte 1 is not used, so this would not cause the verify to fail
            if index != 1 {
                break index;
            }
        };

        instruction.data[index] = instruction.data[index].wrapping_add(12);
        let tx =
            SanitizedTransaction::from_transaction_for_tests(Transaction::new_signed_with_payer(
                &[instruction],
                Some(&mint_keypair.pubkey()),
                &[&mint_keypair],
                Hash::default(),
            ));
        assert!(verify_precompiles(&tx, &feature_set).is_err());
    }

    #[test]
    fn test_verify_precompiles_mixed() {
        let message_arr = b"hello";
        let secp_privkey = libsecp256k1::SecretKey::random(&mut thread_rng());
        let mut secp_instruction = new_secp256k1_instruction(&secp_privkey, message_arr);
        let ed25519_privkey = ed25519_dalek::Keypair::generate(&mut thread_rng());
        let ed25519_instruction = new_ed25519_instruction(&ed25519_privkey, message_arr);

        let mint_keypair = Keypair::new();
        let feature_set = FeatureSet::all_enabled();

        let tx =
            SanitizedTransaction::from_transaction_for_tests(Transaction::new_signed_with_payer(
                &[
                    secp_instruction.clone(),
                    ed25519_instruction.clone(),
                    system_instruction::transfer(&mint_keypair.pubkey(), &Pubkey::new_unique(), 1),
                ],
                Some(&mint_keypair.pubkey()),
                &[&mint_keypair],
                Hash::default(),
            ));
        assert!(verify_precompiles(&tx, &feature_set).is_ok());

        let index = thread_rng().gen_range(0, secp_instruction.data.len());
        secp_instruction.data[index] = secp_instruction.data[index].wrapping_add(12);
        let tx =
            SanitizedTransaction::from_transaction_for_tests(Transaction::new_signed_with_payer(
                &[
                    secp_instruction,
                    ed25519_instruction,
                    system_instruction::transfer(&mint_keypair.pubkey(), &Pubkey::new_unique(), 1),
                ],
                Some(&mint_keypair.pubkey()),
                &[&mint_keypair],
                Hash::default(),
            ));
        assert!(verify_precompiles(&tx, &feature_set).is_err());
    }
}
