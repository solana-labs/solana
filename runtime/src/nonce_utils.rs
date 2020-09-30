use solana_sdk::{
    account::Account,
    account_utils::StateMut,
    fee_calculator::FeeCalculator,
    hash::Hash,
    instruction::CompiledInstruction,
    nonce::{state::Versions, State},
    program_utils::limited_deserialize,
    pubkey::Pubkey,
    system_instruction::SystemInstruction,
    system_program,
    transaction::{Transaction},
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
        } && matches!(limited_deserialize(&maybe_ix.data), Ok(SystemInstruction::AdvanceNonceAccount))
        )
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
    match StateMut::<Versions>::state(acc).map(|v| v.convert_to_current()) {
        Ok(State::Initialized(ref data)) => *hash == data.blockhash,
        _ => false,
    }
}

pub fn fee_calculator_of(account: &Account) -> Option<FeeCalculator> {
    let state = StateMut::<Versions>::state(account)
        .ok()?
        .convert_to_current();
    match state {
        State::Initialized(data) => Some(data.fee_calculator),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use solana_sdk::{
        account_utils::State as AccountUtilsState,
        hash::Hash,
        message::Message,
        nonce::{account::with_test_keyed_account, Account as NonceAccount, State},
        pubkey::Pubkey,
        signature::{Keypair, Signer},
        system_instruction,
        sysvar::{recent_blockhashes::create_test_recent_blockhashes, rent::Rent},
    };
    use std::collections::HashSet;

    fn nonced_transfer_tx() -> (Pubkey, Pubkey, Transaction) {
        let from_keypair = Keypair::new();
        let from_pubkey = from_keypair.pubkey();
        let nonce_keypair = Keypair::new();
        let nonce_pubkey = nonce_keypair.pubkey();
        let instructions = [
            system_instruction::advance_nonce_account(&nonce_pubkey, &nonce_pubkey),
            system_instruction::transfer(&from_pubkey, &nonce_pubkey, 42),
        ];
        let message = Message::new(&instructions, Some(&nonce_pubkey));
        let tx = Transaction::new(&[&from_keypair, &nonce_keypair], message, Hash::default());
        (from_pubkey, nonce_pubkey, tx)
    }

    #[test]
    fn tx_uses_nonce_ok() {
        let (_, _, tx) = nonced_transfer_tx();
        assert!(transaction_uses_durable_nonce(&tx).is_some());
    }

    #[test]
    fn tx_uses_nonce_empty_ix_fail() {
        assert!(transaction_uses_durable_nonce(&Transaction::default()).is_none());
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
        let instructions = [
            system_instruction::transfer(&from_pubkey, &nonce_pubkey, 42),
            system_instruction::advance_nonce_account(&nonce_pubkey, &nonce_pubkey),
        ];
        let message = Message::new(&instructions, Some(&from_pubkey));
        let tx = Transaction::new(&[&from_keypair, &nonce_keypair], message, Hash::default());
        assert!(transaction_uses_durable_nonce(&tx).is_none());
    }

    #[test]
    fn tx_uses_nonce_wrong_first_nonce_ix_fail() {
        let from_keypair = Keypair::new();
        let from_pubkey = from_keypair.pubkey();
        let nonce_keypair = Keypair::new();
        let nonce_pubkey = nonce_keypair.pubkey();
        let instructions = [
            system_instruction::withdraw_nonce_account(
                &nonce_pubkey,
                &nonce_pubkey,
                &from_pubkey,
                42,
            ),
            system_instruction::transfer(&from_pubkey, &nonce_pubkey, 42),
        ];
        let message = Message::new(&instructions, Some(&nonce_pubkey));
        let tx = Transaction::new(&[&from_keypair, &nonce_keypair], message, Hash::default());
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
            signers.insert(nonce_account.signer_key().unwrap());
            let state: State = nonce_account.state().unwrap();
            // New is in Uninitialzed state
            assert_eq!(state, State::Uninitialized);
            let recent_blockhashes = create_test_recent_blockhashes(0);
            let authorized = nonce_account.unsigned_key();
            nonce_account
                .initialize_nonce_account(&authorized, &recent_blockhashes, &Rent::free())
                .unwrap();
            assert!(verify_nonce_account(
                &nonce_account.account.borrow(),
                &recent_blockhashes[0].blockhash,
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
            signers.insert(nonce_account.signer_key().unwrap());
            let state: State = nonce_account.state().unwrap();
            // New is in Uninitialzed state
            assert_eq!(state, State::Uninitialized);
            let recent_blockhashes = create_test_recent_blockhashes(0);
            let authorized = nonce_account.unsigned_key();
            nonce_account
                .initialize_nonce_account(&authorized, &recent_blockhashes, &Rent::free())
                .unwrap();
            assert!(!verify_nonce_account(
                &nonce_account.account.borrow(),
                &recent_blockhashes[1].blockhash,
            ));
        });
    }
}
