#![allow(dead_code)]

use {
    solana_bpf_loader_program::{process_instruction, upgradeable::id},
    solana_program_test::*,
    solana_sdk::{
        account::AccountSharedData,
        account_utils::StateMut,
        bpf_loader_upgradeable::UpgradeableLoaderState,
        instruction::{Instruction, InstructionError},
        pubkey::Pubkey,
        signature::{Keypair, Signer},
        transaction::{Transaction, TransactionError},
    },
};

pub async fn setup_test_context() -> ProgramTestContext {
    let program_test = ProgramTest::new("", id(), Some(process_instruction));
    program_test.start_with_context().await
}

pub async fn assert_ix_error(
    context: &mut ProgramTestContext,
    ixs: &[Instruction],
    additional_payer_keypair: Option<&Keypair>,
    expected_err: InstructionError,
    assertion_failed_msg: &str,
) {
    let client = &mut context.banks_client;
    let fee_payer = &context.payer;
    let recent_blockhash = context.last_blockhash;

    let mut signers = vec![fee_payer];
    if let Some(additional_payer) = additional_payer_keypair {
        signers.push(additional_payer);
    }

    let transaction = Transaction::new_signed_with_payer(
        ixs,
        Some(&fee_payer.pubkey()),
        &signers,
        recent_blockhash,
    );

    assert_eq!(
        client
            .process_transaction(transaction)
            .await
            .unwrap_err()
            .unwrap(),
        TransactionError::InstructionError(0, expected_err),
        "{assertion_failed_msg}",
    );
}

pub async fn add_upgradeable_loader_account(
    context: &mut ProgramTestContext,
    account_address: &Pubkey,
    account_state: &UpgradeableLoaderState,
    account_data_len: usize,
    account_callback: impl Fn(&mut AccountSharedData),
) {
    let rent = context.banks_client.get_rent().await.unwrap();
    let mut account = AccountSharedData::new(
        rent.minimum_balance(account_data_len),
        account_data_len,
        &id(),
    );
    account
        .set_state(account_state)
        .expect("state failed to serialize into account data");
    account_callback(&mut account);
    context.set_account(account_address, &account);
}
