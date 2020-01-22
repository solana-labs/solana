use solana_sdk::{
    account::Account,
    account_utils::State,
    hash::Hash,
    instruction::CompiledInstruction,
    instruction_processor_utils::limited_deserialize,
    nonce_state::NonceState,
    pubkey::Pubkey,
    system_instruction::SystemInstruction,
    system_program,
    transaction::{self, Transaction},
};

pub fn transaction_uses_durable_nonce(tx: &Transaction) -> Option<&CompiledInstruction> {
    let message = tx.message();
    message
        .instructions
        .get(0)
        .filter(|maybe_ix| {
            let prog_id_idx = maybe_ix.program_id_index as usize;
            match message.account_keys.get(prog_id_idx) {
                Some(program_id) => system_program::check_id(&program_id),
                _ => false,
            }
        })
        .filter(|maybe_ix| match limited_deserialize(&maybe_ix.data) {
            Ok(SystemInstruction::AdvanceNonceAccount) => true,
            _ => false,
        })
}

pub fn get_nonce_pubkey_from_instruction<'a>(
    ix: &CompiledInstruction,
    tx: &'a Transaction,
) -> Option<&'a Pubkey> {
    ix.accounts.get(0).and_then(|idx| {
        let idx = *idx as usize;
        tx.message().account_keys.get(idx)
    })
}

pub fn verify_nonce_account(acc: &Account, hash: &Hash) -> bool {
    match acc.state() {
        Ok(NonceState::Initialized(_meta, ref nonce)) => hash == nonce,
        _ => false,
    }
}

pub fn prepare_if_nonce_account(
    account: &mut Account,
    account_pubkey: &Pubkey,
    tx_result: &transaction::Result<()>,
    maybe_nonce: Option<(&Pubkey, &Account)>,
    last_blockhash: &Hash,
) {
    if let Some((nonce_key, nonce_acc)) = maybe_nonce {
        if account_pubkey == nonce_key {
            // Nonce TX failed with an InstructionError. Roll back
            // its account state
            if tx_result.is_err() {
                *account = nonce_acc.clone()
            }
            // Since hash_age_kind is DurableNonce, unwrap is safe here
            if let NonceState::Initialized(meta, _) = account.state().unwrap() {
                account
                    .set_state(&NonceState::Initialized(meta, *last_blockhash))
                    .unwrap();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use solana_sdk::{
        account::Account,
        hash::Hash,
        instruction::InstructionError,
        nonce_state::{with_test_keyed_account, Meta, NonceAccount},
        pubkey::Pubkey,
        signature::{Keypair, KeypairUtil},
        system_instruction,
        sysvar::{recent_blockhashes::create_test_recent_blockhashes, rent::Rent},
    };
    use std::collections::HashSet;

    fn nonced_transfer_tx() -> (Pubkey, Pubkey, Transaction) {
        let from_keypair = Keypair::new();
        let from_pubkey = from_keypair.pubkey();
        let nonce_keypair = Keypair::new();
        let nonce_pubkey = nonce_keypair.pubkey();
        let tx = Transaction::new_signed_instructions(
            &[&from_keypair, &nonce_keypair],
            vec![
                system_instruction::advance_nonce_account(&nonce_pubkey, &nonce_pubkey),
                system_instruction::transfer(&from_pubkey, &nonce_pubkey, 42),
            ],
            Hash::default(),
        );
        (from_pubkey, nonce_pubkey, tx)
    }

    #[test]
    fn tx_uses_nonce_ok() {
        let (_, _, tx) = nonced_transfer_tx();
        assert!(transaction_uses_durable_nonce(&tx).is_some());
    }

    #[test]
    fn tx_uses_nonce_empty_ix_fail() {
        let tx =
            Transaction::new_signed_instructions(&[&Keypair::new(); 0], vec![], Hash::default());
        assert!(transaction_uses_durable_nonce(&tx).is_none());
    }

    #[test]
    fn tx_uses_nonce_bad_prog_id_idx_fail() {
        let (_, _, mut tx) = nonced_transfer_tx();
        tx.message.instructions.get_mut(0).unwrap().program_id_index = 255u8;
        assert!(transaction_uses_durable_nonce(&tx).is_none());
    }

    #[test]
    fn tx_uses_nonce_first_prog_id_not_nonce_fail() {
        let from_keypair = Keypair::new();
        let from_pubkey = from_keypair.pubkey();
        let nonce_keypair = Keypair::new();
        let nonce_pubkey = nonce_keypair.pubkey();
        let tx = Transaction::new_signed_instructions(
            &[&from_keypair, &nonce_keypair],
            vec![
                system_instruction::transfer(&from_pubkey, &nonce_pubkey, 42),
                system_instruction::advance_nonce_account(&nonce_pubkey, &nonce_pubkey),
            ],
            Hash::default(),
        );
        assert!(transaction_uses_durable_nonce(&tx).is_none());
    }

    #[test]
    fn tx_uses_nonce_wrong_first_nonce_ix_fail() {
        let from_keypair = Keypair::new();
        let from_pubkey = from_keypair.pubkey();
        let nonce_keypair = Keypair::new();
        let nonce_pubkey = nonce_keypair.pubkey();
        let tx = Transaction::new_signed_instructions(
            &[&from_keypair, &nonce_keypair],
            vec![
                system_instruction::withdraw_nonce_account(
                    &nonce_pubkey,
                    &nonce_pubkey,
                    &from_pubkey,
                    42,
                ),
                system_instruction::transfer(&from_pubkey, &nonce_pubkey, 42),
            ],
            Hash::default(),
        );
        assert!(transaction_uses_durable_nonce(&tx).is_none());
    }

    #[test]
    fn get_nonce_pub_from_ix_ok() {
        let (_, nonce_pubkey, tx) = nonced_transfer_tx();
        let nonce_ix = transaction_uses_durable_nonce(&tx).unwrap();
        assert_eq!(
            get_nonce_pubkey_from_instruction(&nonce_ix, &tx),
            Some(&nonce_pubkey),
        );
    }

    #[test]
    fn get_nonce_pub_from_ix_no_accounts_fail() {
        let (_, _, tx) = nonced_transfer_tx();
        let nonce_ix = transaction_uses_durable_nonce(&tx).unwrap();
        let mut nonce_ix = nonce_ix.clone();
        nonce_ix.accounts.clear();
        assert_eq!(get_nonce_pubkey_from_instruction(&nonce_ix, &tx), None,);
    }

    #[test]
    fn get_nonce_pub_from_ix_bad_acc_idx_fail() {
        let (_, _, tx) = nonced_transfer_tx();
        let nonce_ix = transaction_uses_durable_nonce(&tx).unwrap();
        let mut nonce_ix = nonce_ix.clone();
        nonce_ix.accounts[0] = 255u8;
        assert_eq!(get_nonce_pubkey_from_instruction(&nonce_ix, &tx), None,);
    }

    #[test]
    fn verify_nonce_ok() {
        with_test_keyed_account(42, true, |nonce_account| {
            let mut signers = HashSet::new();
            signers.insert(nonce_account.signer_key().unwrap().clone());
            let state: NonceState = nonce_account.state().unwrap();
            // New is in Uninitialzed state
            assert_eq!(state, NonceState::Uninitialized);
            let recent_blockhashes = create_test_recent_blockhashes(0);
            let authorized = nonce_account.unsigned_key().clone();
            nonce_account
                .initialize_nonce_account(&authorized, &recent_blockhashes, &Rent::free())
                .unwrap();
            assert!(verify_nonce_account(
                &nonce_account.account.borrow(),
                &recent_blockhashes[0]
            ));
        });
    }

    #[test]
    fn verify_nonce_bad_acc_state_fail() {
        with_test_keyed_account(42, true, |nonce_account| {
            assert!(!verify_nonce_account(
                &nonce_account.account.borrow(),
                &Hash::default()
            ));
        });
    }

    #[test]
    fn verify_nonce_bad_query_hash_fail() {
        with_test_keyed_account(42, true, |nonce_account| {
            let mut signers = HashSet::new();
            signers.insert(nonce_account.signer_key().unwrap().clone());
            let state: NonceState = nonce_account.state().unwrap();
            // New is in Uninitialzed state
            assert_eq!(state, NonceState::Uninitialized);
            let recent_blockhashes = create_test_recent_blockhashes(0);
            let authorized = nonce_account.unsigned_key().clone();
            nonce_account
                .initialize_nonce_account(&authorized, &recent_blockhashes, &Rent::free())
                .unwrap();
            assert!(!verify_nonce_account(
                &nonce_account.account.borrow(),
                &recent_blockhashes[1]
            ));
        });
    }

    fn create_accounts_prepare_if_nonce_account() -> (Pubkey, Account, Account, Hash) {
        let stored_nonce = Hash::default();
        let account = Account::new_data(
            42,
            &NonceState::Initialized(Meta::new(&Pubkey::default()), stored_nonce),
            &system_program::id(),
        )
        .unwrap();
        let pre_account = Account {
            lamports: 43,
            ..account.clone()
        };
        (
            Pubkey::default(),
            pre_account,
            account,
            Hash::new(&[1u8; 32]),
        )
    }

    fn run_prepare_if_nonce_account_test(
        account: &mut Account,
        account_pubkey: &Pubkey,
        tx_result: &transaction::Result<()>,
        maybe_nonce: Option<(&Pubkey, &Account)>,
        last_blockhash: &Hash,
        expect_account: &Account,
    ) -> bool {
        // Verify expect_account's relationship
        match maybe_nonce {
            Some((nonce_pubkey, _nonce_account))
                if nonce_pubkey == account_pubkey && tx_result.is_ok() =>
            {
                assert_ne!(expect_account, account)
            }
            Some((nonce_pubkey, nonce_account)) if nonce_pubkey == account_pubkey => {
                assert_ne!(expect_account, nonce_account)
            }
            _ => assert_eq!(expect_account, account),
        }

        prepare_if_nonce_account(
            account,
            account_pubkey,
            tx_result,
            maybe_nonce,
            last_blockhash,
        );
        expect_account == account
    }

    #[test]
    fn test_prepare_if_nonce_account_expected() {
        let (pre_account_pubkey, pre_account, mut post_account, last_blockhash) =
            create_accounts_prepare_if_nonce_account();
        let post_account_pubkey = pre_account_pubkey;

        let mut expect_account = post_account.clone();
        expect_account
            .set_state(&NonceState::Initialized(
                Meta::new(&Pubkey::default()),
                last_blockhash,
            ))
            .unwrap();

        assert!(run_prepare_if_nonce_account_test(
            &mut post_account,
            &post_account_pubkey,
            &Ok(()),
            Some((&pre_account_pubkey, &pre_account)),
            &last_blockhash,
            &expect_account,
        ));
    }

    #[test]
    fn test_prepare_if_nonce_account_not_nonce_tx() {
        let (pre_account_pubkey, _pre_account, _post_account, last_blockhash) =
            create_accounts_prepare_if_nonce_account();
        let post_account_pubkey = pre_account_pubkey;

        let mut post_account = Account::default();
        let expect_account = post_account.clone();
        assert!(run_prepare_if_nonce_account_test(
            &mut post_account,
            &post_account_pubkey,
            &Ok(()),
            None,
            &last_blockhash,
            &expect_account,
        ));
    }

    #[test]
    fn test_prepare_if_nonce_naccount_ot_nonce_pubkey() {
        let (pre_account_pubkey, pre_account, mut post_account, last_blockhash) =
            create_accounts_prepare_if_nonce_account();

        let expect_account = post_account.clone();
        // Wrong key
        assert!(run_prepare_if_nonce_account_test(
            &mut post_account,
            &Pubkey::new(&[1u8; 32]),
            &Ok(()),
            Some((&pre_account_pubkey, &pre_account)),
            &last_blockhash,
            &expect_account,
        ));
    }

    #[test]
    fn test_prepare_if_nonce_account_tx_error() {
        let (pre_account_pubkey, pre_account, mut post_account, last_blockhash) =
            create_accounts_prepare_if_nonce_account();
        let post_account_pubkey = pre_account_pubkey;

        let mut expect_account = pre_account.clone();
        expect_account
            .set_state(&NonceState::Initialized(
                Meta::new(&Pubkey::default()),
                last_blockhash,
            ))
            .unwrap();

        assert!(run_prepare_if_nonce_account_test(
            &mut post_account,
            &post_account_pubkey,
            &Err(transaction::TransactionError::InstructionError(
                0,
                InstructionError::InvalidArgument.into(),
            )),
            Some((&pre_account_pubkey, &pre_account)),
            &last_blockhash,
            &expect_account,
        ));
    }
}
